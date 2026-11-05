use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use serde::Serialize;

use super::Ctx;
use crate::agent;
use crate::git;
use crate::runtime;
use crate::store::digest::Digest;
use crate::store::journal::{self, Kind};
use crate::store::lock;
use crate::store::sessions::{self, Session};
use crate::store::todo::{Repo, Status, Todo};
use crate::util::local;

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub id: u64,
    /// repos が複数ある時に選ぶ (name)
    #[arg(long, value_name = "NAME")]
    pub repo: Option<String>,
    /// 表示せず、このプロセスを claude に差し替える
    #[arg(long)]
    pub exec: bool,
}

#[derive(Serialize)]
struct Out {
    id: u64,
    session_id: String,
    cwd: PathBuf,
    worktree: Option<PathBuf>,
    command: Option<String>,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let launch;
    let out;
    {
        let _g = lock::lock_todo(&ctx.store, a.id)?;
        let mut t = ctx.load(a.id)?;
        let repo = pick_repo(&t, a.repo.as_deref())?.cloned();

        let (cwd, worktree) = match &repo {
            Some(r) => {
                let dir = worktree_dir(r, a.id);
                let created =
                    git::worktree_add(&r.path, &dir, &format!("todo/{}", a.id), &r.base_branch)?;
                if created && !ctx.json {
                    eprintln!("worktree created: {}", dir.display());
                }
                (dir.clone(), Some(dir))
            }
            None => (std::env::current_dir()?, None),
        };

        let session_id = uuid::Uuid::new_v4().to_string();
        let context_path = ctx.store.context(a.id);
        std::fs::write(&context_path, context_md(ctx, &t)?)?;

        let name = agent::session_name(a.id, repo.as_ref().map(|r| r.name.as_str()));
        launch = agent::claude_launch(&cwd, a.id, &session_id, &name, &context_path);
        let rt = runtime::from_env()?;
        let spawned = rt.spawn(&launch)?;

        let spath = ctx.store.sessions(a.id);
        let mut list = sessions::load(&spath)?;
        list.push(Session {
            session_id: session_id.clone(),
            runtime: spawned.reference,
            repo: repo.as_ref().map(|r| r.name.clone()),
            worktree: worktree.clone(),
            cwd: cwd.clone(),
            started_at: ctx.now,
            ended_at: None,
            log_bytes: 0,
        });
        sessions::save(&spath, &list)?;

        if matches!(t.meta.status, Status::Inbox | Status::Planning) {
            t.meta.status = Status::Active;
        }
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
                repo.as_ref()
                    .map(|r| format!(" repo={}", r.name))
                    .unwrap_or_default(),
                worktree
                    .as_ref()
                    .map(|w| format!(" worktree={}", w.display()))
                    .unwrap_or_default()
            ),
        )?;
        out = Out {
            id: a.id,
            session_id,
            cwd,
            worktree,
            command: spawned.shell_command,
        };
    }

    if a.exec {
        return exec(&launch);
    }
    if ctx.json {
        ctx.print_json(&out)
    } else {
        match &out.command {
            Some(c) => println!("{c}"),
            None => println!("session {} spawned", out.session_id),
        }
        Ok(())
    }
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

/// `<repo>/../<repo名>-wt/todo-<id>`
fn worktree_dir(r: &Repo, id: u64) -> PathBuf {
    let parent = r.path.parent().unwrap_or(&r.path);
    parent
        .join(format!("{}-wt", r.name))
        .join(format!("todo-{id}"))
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
         進捗の区切りごとに `kotoshiro note`、checklist は `kotoshiro check <n>`、\
         スコープ外の障害は `kotoshiro new --blocks $KOTOSHIRO_TODO`。`kotoshiro status ... done` は打たない。\n",
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
