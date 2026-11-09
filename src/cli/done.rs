//! 完了処理。条件判定 → PR 収集 → issue コメント → 片付け → status done。

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use serde::Serialize;

use super::Ctx;
use crate::runtime::{self, Workspace};
use crate::store::digest::Digest;
use crate::store::journal::{self, Kind};
use crate::store::lock;
use crate::store::sessions;
use crate::store::todo::{Pr, Status, Todo};
use crate::summarizer;
use crate::util::{local, split_issue};
use crate::{gh, git};

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub id: u64,
    /// 条件違反と gh / git の失敗を警告に落として進める
    #[arg(long)]
    pub force: bool,
    /// issue コメントの確認を省く
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// issue にコメントしない
    #[arg(long)]
    pub no_comment: bool,
    /// worktree とブランチを残す
    #[arg(long)]
    pub keep_worktree: bool,
    /// 条件判定と PR 収集だけ行い、何も書かない
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Serialize)]
struct Report {
    id: u64,
    dry_run: bool,
    ok: bool,
    problems: Vec<String>,
    prs: Vec<Pr>,
    comment: Option<String>,
    comment_posted: bool,
    worktrees_removed: Vec<PathBuf>,
    workspace_closed: bool,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let _g = lock::lock_todo(&ctx.store, a.id)?;
    let mut t = ctx.load(a.id)?;
    if t.meta.status == Status::Done {
        bail!("#{} is already done", a.id);
    }
    if ctx.json && !a.yes && !a.no_comment && t.meta.issue.is_some() {
        bail!("--json cannot confirm the issue comment; pass --yes or --no-comment");
    }

    let mut problems = Vec::new();
    let left = t.checklist().iter().filter(|i| !i.done).count();
    if left > 0 {
        problems.push(format!("{left} checklist item(s) left"));
    }
    let map = ctx.status_map()?;
    let open: Vec<String> = t
        .meta
        .blocked_by
        .iter()
        .filter(|b| !matches!(map.get(b), Some(Status::Done)))
        .map(|b| format!("#{b}"))
        .collect();
    if !open.is_empty() {
        problems.push(format!("blocked by {}", open.join(", ")));
    }
    let sess = sessions::load(&ctx.store.sessions(a.id))?;
    let live: Vec<&str> = sess
        .iter()
        .filter(|s| s.is_live())
        .map(|s| journal::short(&s.session_id))
        .collect();
    if !live.is_empty() {
        problems.push(format!(
            "{} live session(s): {} (end them, or --force)",
            live.len(),
            live.join(", ")
        ));
    }

    // PR 収集。ブランチが無い repo は手を付けていないので見ない
    let mut found: Vec<(String, gh::Pr)> = Vec::new();
    for r in &t.meta.repos {
        let branch = r.branch(a.id);
        if !git::branch_exists(&r.path, &branch) {
            continue;
        }
        match gh::name_with_owner(&r.path).and_then(|nwo| gh::prs_for_branch(&nwo, &branch)) {
            Ok(list) if list.is_empty() => {
                problems.push(format!("{}: no PR for branch {branch}", r.name));
            }
            Ok(list) => {
                let open = list.iter().filter(|p| p.state == "OPEN").count();
                if open > 0 {
                    problems.push(format!("{}: {open} open PR(s)", r.name));
                }
                if !list.iter().any(|p| p.state == "MERGED") {
                    problems.push(format!("{}: no merged PR", r.name));
                }
                found.extend(list.into_iter().map(|p| (r.name.clone(), p)));
            }
            Err(e) => problems.push(format!("{}: gh failed: {e:#}", r.name)),
        }
    }
    let prs: Vec<Pr> = found
        .iter()
        .map(|(repo, p)| Pr {
            repo: repo.clone(),
            number: p.number,
            state: p.state.clone(),
        })
        .collect();

    let ok = problems.is_empty();
    let mut report = Report {
        id: a.id,
        dry_run: a.dry_run,
        ok,
        problems,
        prs,
        comment: None,
        comment_posted: false,
        worktrees_removed: Vec::new(),
        workspace_closed: false,
    };
    if a.dry_run {
        return output(ctx, &report, &t);
    }
    if !ok {
        if !a.force {
            bail!(
                "cannot finish #{}: {} (use --force)",
                a.id,
                report.problems.join("; ")
            );
        }
        for p in &report.problems {
            eprintln!("warning: {p}");
        }
    }

    // issue コメント
    if let (Some(issue), false) = (&t.meta.issue, a.no_comment) {
        let (nwo, number) = split_issue(issue)?;
        let body = generate_comment(ctx, &t, &found)?;
        if !ctx.json {
            println!("--- comment for {issue} ---\n{body}\n---");
        }
        if a.yes || confirm(&format!("post this comment to {issue}? [y/N] "))? {
            gh::issue_comment(&nwo, number, &body)?;
            report.comment_posted = true;
        } else {
            eprintln!("comment not posted");
        }
        report.comment = Some(body);
    }

    // 片付け
    if !a.keep_worktree {
        for r in &t.meta.repos {
            let dir = ctx.config.worktree_dir(r, a.id);
            if dir.is_dir() {
                match git::worktree_remove(&r.path, &dir, a.force) {
                    Ok(()) => report.worktrees_removed.push(dir),
                    Err(e) => eprintln!("warning: {e:#} (keep it, or --force)"),
                }
            }
            let branch = r.branch(a.id);
            if git::branch_exists(&r.path, &branch) {
                if let Err(e) = git::branch_delete(&r.path, &branch) {
                    eprintln!("warning: {e:#}");
                }
            }
        }
    }
    let rt = runtime::select(&ctx.config.runtime(None))?;
    let ws = Workspace {
        id: a.id,
        title: &t.meta.title,
        repo: None,
        focus: false,
    };
    match rt.close_workspace(&ws) {
        Ok(closed) => report.workspace_closed = closed,
        Err(e) => eprintln!("warning: {e:#}"),
    }

    let old = t.meta.status;
    t.meta.prs = report.prs.clone();
    t.meta.status = Status::Done;
    t.touch(ctx.now);
    ctx.save(&t)?;
    let mut note = format!("{old} -> done");
    if !report.prs.is_empty() {
        let list: Vec<String> = report
            .prs
            .iter()
            .map(|p| format!("{}#{} {}", p.repo, p.number, p.state))
            .collect();
        note.push_str(&format!("\nprs: {}", list.join(", ")));
    }
    for w in &report.worktrees_removed {
        note.push_str(&format!("\nworktree removed: {}", w.display()));
    }
    if report.workspace_closed {
        note.push_str("\nworkspace closed");
    }
    if let (Some(c), true) = (&report.comment, report.comment_posted) {
        note.push_str(&format!(
            "\nissue comment posted to {}:\n{c}",
            t.meta.issue.as_deref().unwrap_or_default()
        ));
    }
    journal::append(
        &ctx.store.journal(a.id),
        ctx.now,
        Kind::Done,
        crate::resolve::env_session().as_deref(),
        &note,
    )?;
    output(ctx, &report, &t)
}

fn output(ctx: &Ctx, r: &Report, t: &Todo) -> Result<()> {
    if ctx.json {
        return ctx.print_json(r);
    }
    if r.dry_run {
        println!(
            "dry-run: #{} {} {}",
            r.id,
            t.meta.title,
            if r.ok {
                "can be finished"
            } else {
                "is not ready"
            }
        );
    } else {
        println!("#{} done: {}", r.id, t.meta.title);
    }
    for p in &r.problems {
        println!("  ! {p}");
    }
    for p in &r.prs {
        println!("  pr: {}#{} {}", p.repo, p.number, p.state);
    }
    if r.comment_posted {
        println!("  issue comment posted");
    }
    for w in &r.worktrees_removed {
        println!("  worktree removed: {}", w.display());
    }
    if r.workspace_closed {
        println!("  workspace closed");
    }
    Ok(())
}

const PROMPT: &str =
    "以下は作業記録です。GitHub issue に投稿する完了コメントを日本語で書いてください。\
読み手は開発者でないメンバーも含みます。\
内容: やったことの要点 (箇条書き 3〜6 行)、PR があればその一覧、残課題や注意点があれば 1〜2 行。\
です・ます調。前置きや挨拶は書かない。Markdown 可。コメント本文だけを出力する。";

fn generate_comment(ctx: &Ctx, t: &Todo, prs: &[(String, gh::Pr)]) -> Result<String> {
    let m = &t.meta;
    let mut input = format!("# TODO #{}: {}\n", m.id, m.title);
    if let Some(i) = &m.issue {
        input.push_str(&format!("issue: {i}\n"));
    }
    input.push_str("\n## checklist\n");
    for it in t.checklist() {
        input.push_str(&format!(
            "- [{}] {}\n",
            if it.done { 'x' } else { ' ' },
            it.text
        ));
    }
    if !prs.is_empty() {
        input.push_str("\n## PR\n");
        for (repo, p) in prs {
            input.push_str(&format!(
                "- {repo}#{} {} ({}) {}\n",
                p.number, p.title, p.state, p.url
            ));
        }
    }
    if let Some(d) = Digest::load(&ctx.store.digest(m.id))? {
        input.push_str(&format!(
            "\n## digest ({})\n{}\n\nいま: {}\n次: {}\n詰まり: {}\n",
            local(d.head.updated_at),
            d.line,
            d.now,
            d.next,
            d.stuck
        ));
    }
    let tail = journal::tail(&ctx.store.journal(m.id), 40)?;
    if !tail.is_empty() {
        input.push_str("\n## journal\n");
        for e in tail {
            input.push_str(&format!("### {} [{}]\n{}\n\n", local(e.ts), e.kind, e.text));
        }
    }
    let s = summarizer::select(&ctx.config);
    s.summarize(PROMPT, &input)
        .with_context(|| format!("summarizer `{}`", s.name()))
        .map(|c| c.trim().to_owned())
}

fn confirm(prompt: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("stdin is not a terminal; pass --yes (or --no-comment)");
    }
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}
