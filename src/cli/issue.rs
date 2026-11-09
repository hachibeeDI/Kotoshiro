//! `issue create`: TODO から GitHub issue を起こして link する。

use std::io::{IsTerminal, Write};

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use serde::Serialize;

use super::Ctx;
use crate::gh;
use crate::store::journal::{self, Kind};
use crate::store::lock;

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// title と本文で gh issue create し、できた issue を link する
    Create(CreateArgs),
}

#[derive(ClapArgs, Debug)]
pub struct CreateArgs {
    pub id: u64,
    /// 作成先 (owner/repo)。省略時は TODO の repos が 1 つならそれ
    #[arg(long, value_name = "owner/repo")]
    pub repo: Option<String>,
    /// 内容の確認を省く
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Serialize)]
struct Out {
    id: u64,
    issue: String,
    url: String,
}

pub fn run(ctx: &Ctx, c: Cmd) -> Result<()> {
    match c {
        Cmd::Create(a) => create(ctx, a),
    }
}

fn create(ctx: &Ctx, a: CreateArgs) -> Result<()> {
    let _g = lock::lock_todo(&ctx.store, a.id)?;
    let mut t = ctx.load(a.id)?;
    if let Some(i) = &t.meta.issue {
        bail!("#{} is already linked to {i} (use `link` to change)", a.id);
    }
    let nwo = match (&a.repo, t.meta.repos.as_slice()) {
        (Some(r), _) => r.clone(),
        (None, [one]) => gh::name_with_owner(&one.path)?,
        (None, []) => bail!("#{} has no repos; pass --repo owner/repo", a.id),
        (None, _) => bail!("#{} has several repos; pass --repo owner/repo", a.id),
    };
    if !nwo.contains('/') {
        bail!("--repo must be owner/repo, got `{nwo}`");
    }
    let body = {
        let b = t.body.trim();
        if b.is_empty() {
            format!("(kotoshiro TODO #{})", a.id)
        } else {
            b.to_owned()
        }
    };
    if ctx.json && !a.yes {
        bail!("--json cannot confirm; pass --yes");
    }
    if !ctx.json {
        println!("--- issue for {nwo} ---\n# {}\n\n{body}\n---", t.meta.title);
    }
    if !a.yes && !confirm("create this issue? [y/N] ")? {
        bail!("aborted");
    }
    let url = gh::issue_create(&nwo, &t.meta.title, &body)?;
    let number: u64 = url
        .rsplit('/')
        .next()
        .and_then(|n| n.parse().ok())
        .with_context(|| format!("unexpected gh output: {url}"))?;
    let issue = format!("{nwo}#{number}");
    t.meta.issue = Some(issue.clone());
    t.touch(ctx.now);
    ctx.save(&t)?;
    journal::append(
        &ctx.store.journal(a.id),
        ctx.now,
        Kind::Status,
        crate::resolve::env_session().as_deref(),
        &format!("issue created: {url}"),
    )?;
    if ctx.json {
        ctx.print_json(&Out {
            id: a.id,
            issue,
            url,
        })
    } else {
        println!("#{} linked to {issue} ({url})", a.id);
        Ok(())
    }
}

fn confirm(prompt: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("stdin is not a terminal; pass --yes");
    }
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}
