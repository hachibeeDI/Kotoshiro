use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use serde::Serialize;

use super::Ctx;
use crate::agent;
use crate::git;
use crate::runtime::{self, Spawn, Workspace};
use crate::store::digest::Digest;
use crate::store::journal::{self, Kind};
use crate::store::lock;
use crate::store::sessions::{self, Session, SessionRef};
use crate::store::todo::{Repo, Status, Todo};
use crate::util::local;

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub id: u64,
    /// repos が複数ある時に選ぶ (name)
    #[arg(long, value_name = "NAME")]
    pub repo: Option<String>,
    /// claude を起動せず、記録だけして起動コマンドを表示する (--json も同じ扱い)
    #[arg(long, conflicts_with = "dry_run")]
    pub print: bool,
    /// 何も書き込まず、何が起きるかだけ表示する
    #[arg(long)]
    pub dry_run: bool,
    /// Runtime adapter (既定: KOTOSHIRO_RUNTIME、無ければ manual)
    #[arg(long, value_name = "NAME")]
    pub runtime: Option<String>,
    /// herdr: 新しい workspace / tab にフォーカスを移さない
    #[arg(long)]
    pub no_focus: bool,
}

#[derive(Serialize)]
struct Plan {
    id: u64,
    dry_run: bool,
    runtime: String,
    repo: Option<String>,
    cwd: PathBuf,
    worktree: Option<PathBuf>,
    worktree_exists: bool,
    context: PathBuf,
    status_from: Status,
    status_to: Status,
    session_id: String,
    command: String,
}

const DRY_RUN_SESSION: &str = "<session-id>";

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let rt = match &a.runtime {
        Some(n) => runtime::select(n)?,
        None => runtime::from_env()?,
    };
    let launch;
    let title;
    let repo_name;
    {
        let _g = lock::lock_todo(&ctx.store, a.id)?;
        let mut t = ctx.load(a.id)?;
        let repo = pick_repo(&t, a.repo.as_deref())?.cloned();

        let (cwd, worktree) = match &repo {
            Some(r) => {
                let dir = r.worktree_dir(a.id);
                (dir.clone(), Some(dir))
            }
            None => (std::env::current_dir()?, None),
        };
        let worktree_exists = worktree.as_deref().is_some_and(|w| w.is_dir());
        let session_id = if a.dry_run {
            DRY_RUN_SESSION.to_owned()
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        let context_path = ctx.store.context(a.id);
        let name = agent::session_name(a.id, repo.as_ref().map(|r| r.name.as_str()));
        launch = agent::claude_launch(&cwd, a.id, &session_id, &name, &context_path);
        let status_to = match t.meta.status {
            Status::Inbox | Status::Planning => Status::Active,
            s => s,
        };
        let plan = Plan {
            id: a.id,
            dry_run: a.dry_run,
            runtime: rt.name().to_owned(),
            repo: repo.as_ref().map(|r| r.name.clone()),
            cwd: cwd.clone(),
            worktree: worktree.clone(),
            worktree_exists,
            context: context_path.clone(),
            status_from: t.meta.status,
            status_to,
            session_id: session_id.clone(),
            command: runtime::manual::shell_command(&launch),
        };

        if a.dry_run {
            return if ctx.json {
                ctx.print_json(&plan)
            } else {
                print!("{}", render_plan(&plan, &t, repo.as_ref()));
                Ok(())
            };
        }

        if let Some((r, dir)) = repo.as_ref().zip(worktree.as_ref()) {
            let created = git::worktree_add(&r.path, dir, &r.branch(a.id), &r.base_branch)?;
            if created && !ctx.json {
                eprintln!("worktree created: {}", dir.display());
            }
        }
        std::fs::write(&context_path, context_md(ctx, &t)?)?;

        let spath = ctx.store.sessions(a.id);
        let mut list = sessions::load(&spath)?;
        list.push(Session {
            session_id: session_id.clone(),
            runtime: SessionRef {
                runtime: rt.name().to_owned(),
                reference: String::new(),
            },
            repo: plan.repo.clone(),
            worktree: worktree.clone(),
            cwd: cwd.clone(),
            started_at: ctx.now,
            ended_at: None,
            log_bytes: 0,
        });
        sessions::save(&spath, &list)?;

        t.meta.status = status_to;
        t.touch(ctx.now);
        ctx.save(&t)?;
        journal::append(
            &ctx.store.journal(a.id),
            ctx.now,
            Kind::Start,
            Some(&session_id),
            &format!(
                "session started via {}{}{}",
                rt.name(),
                plan.repo
                    .as_ref()
                    .map(|r| format!(" repo={r}"))
                    .unwrap_or_default(),
                worktree
                    .as_ref()
                    .map(|w| format!(" worktree={}", w.display()))
                    .unwrap_or_default()
            ),
        )?;

        if a.print || ctx.json {
            return if ctx.json {
                ctx.print_json(&plan)
            } else {
                println!("{}", plan.command);
                Ok(())
            };
        }
        title = t.meta.title.clone();
        repo_name = plan.repo.clone();
    }

    // ロックを手放してから起動する (herdr の agent start は数十秒待つ)
    let ws = Workspace {
        id: a.id,
        title: &title,
        repo: repo_name.as_deref(),
        focus: !a.no_focus,
    };
    let session_id = launch
        .env
        .iter()
        .find(|(k, _)| k == agent::ENV_SESSION)
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    match rt.spawn(&launch, &ws) {
        Ok(Spawn::Exec) => exec(&launch),
        Ok(Spawn::Started(r)) => {
            update_session(ctx, a.id, &session_id, |s| s.runtime = r.clone())?;
            println!(
                "#{} started: {} {} (session {})",
                a.id,
                r.runtime,
                r.reference,
                journal::short(&session_id)
            );
            Ok(())
        }
        Err(e) => {
            // 起動できなかったセッションを live のまま残さない
            update_session(ctx, a.id, &session_id, |s| s.ended_at = Some(ctx.now))?;
            Err(e).with_context(|| format!("runtime `{}` failed to start the session", rt.name()))
        }
    }
}

fn update_session(
    ctx: &Ctx,
    id: u64,
    session_id: &str,
    f: impl FnOnce(&mut Session),
) -> Result<()> {
    let _g = lock::lock_todo(&ctx.store, id)?;
    let spath = ctx.store.sessions(id);
    let mut list = sessions::load(&spath)?;
    if let Some(s) = list.iter_mut().find(|s| s.session_id == session_id) {
        f(s);
        sessions::save(&spath, &list)?;
    }
    Ok(())
}

fn render_plan(p: &Plan, t: &Todo, repo: Option<&Repo>) -> String {
    let mut o = String::from("dry-run: nothing is written\n");
    o.push_str(&format!("todo:      #{} {}\n", p.id, t.meta.title));
    o.push_str(&format!("runtime:   {}\n", p.runtime));
    match repo {
        Some(r) => o.push_str(&format!(
            "repo:      {} ({}, base {})\n",
            r.name,
            r.path.display(),
            r.base_branch
        )),
        None => o.push_str("repo:      (none; start in current directory)\n"),
    }
    if let Some(w) = &p.worktree {
        o.push_str(&format!(
            "worktree:  {} ({})\n",
            w.display(),
            if p.worktree_exists { "reuse" } else { "create" }
        ));
    }
    o.push_str(&format!("context:   {}\n", p.context.display()));
    o.push_str(&format!(
        "status:    {} -> {}\n",
        p.status_from, p.status_to
    ));
    o.push_str(&format!("command:   {}\n", p.command));
    o
}

fn pick_repo<'a>(t: &'a Todo, name: Option<&str>) -> Result<Option<&'a Repo>> {
    let repos = &t.meta.repos;
    match (repos.len(), name) {
        (0, None) => Ok(None),
        (0, Some(_)) => bail!("#{} has no repos (add with `new --repo`)", t.meta.id),
        (1, None) => Ok(repos.first()),
        (_, None) => {
            let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
            bail!(
                "#{} has several repos; pass --repo <{}>",
                t.meta.id,
                names.join("|")
            )
        }
        (_, Some(n)) => repos
            .iter()
            .find(|r| r.name == n)
            .map(Some)
            .with_context(|| format!("repo `{n}` not in #{}", t.meta.id)),
    }
}

fn context_md(ctx: &Ctx, t: &Todo) -> Result<String> {
    let m = &t.meta;
    let mut o = format!("# kotoshiro TODO #{}: {}\n\n", m.id, m.title);
    o.push_str(&format!("status: {}\n", m.status));
    if let Some(i) = &m.issue {
        o.push_str(&format!("issue: {i}\n"));
    }
    if !m.repos.is_empty() {
        let rs: Vec<String> = m
            .repos
            .iter()
            .map(|r| format!("{} ({})", r.name, r.path.display()))
            .collect();
        o.push_str(&format!("repos: {}\n", rs.join(", ")));
    }
    o.push_str(
        "\nこの作業は kotoshiro の TODO として管理されている。skill `kotoshiro` の規則に従うこと: \
         計画が固まったら `kotoshiro plan -` に箇条書きを流す、進捗の区切りごとに `kotoshiro note`、\
         checklist は `kotoshiro check <n>`、スコープ外の障害は `kotoshiro new --blocks $KOTOSHIRO_TODO`。\
         `kotoshiro status ... done` は打たない。\n",
    );
    o.push_str("\n## todo\n");
    o.push_str(t.body.trim());
    o.push('\n');
    if let Some(d) = Digest::load(&ctx.store.digest(m.id))? {
        o.push_str(&format!(
            "\n## digest ({})\n{}\n\nいま: {}\n次: {}\n詰まり: {}\n",
            local(d.head.updated_at),
            d.line,
            d.now,
            d.next,
            d.stuck
        ));
    }
    let tail = journal::tail(&ctx.store.journal(m.id), 10)?;
    if !tail.is_empty() {
        o.push_str("\n## journal (直近)\n");
        for e in tail {
            o.push_str(&format!("### {} [{}]\n{}\n\n", local(e.ts), e.kind, e.text));
        }
    }
    Ok(o)
}

fn exec(l: &agent::Launch) -> Result<()> {
    let mut c = Command::new(&l.argv[0]);
    c.args(&l.argv[1..]).current_dir(&l.cwd);
    for (k, v) in &l.env {
        c.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = c.exec();
        bail!("exec {}: {err}", l.argv[0]);
    }
    #[cfg(not(unix))]
    {
        let st = c.status()?;
        std::process::exit(st.code().unwrap_or(1));
    }
}
