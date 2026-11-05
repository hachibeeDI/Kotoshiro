use std::io::Read;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;

use super::Ctx;
use crate::git;
use crate::store::journal::{self, Kind};
use crate::store::todo::{Repo, Todo};
use crate::store::{counter, graph, lock};
use crate::util::parse_issue;

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub title: String,
    /// GitHub issue (org/repo#N)。無ければ単独 TODO
    #[arg(long, value_name = "org/repo#N")]
    pub issue: Option<String>,
    /// 作業対象リポジトリのパス。複数可
    #[arg(long, value_name = "PATH")]
    pub repo: Vec<PathBuf>,
    /// この TODO が block する TODO (相手は blocked になる)
    #[arg(long, value_name = "ID")]
    pub blocks: Option<u64>,
    /// 親 (Epic)
    #[arg(long, value_name = "ID")]
    pub parent: Option<u64>,
    /// 本文。`-` で stdin
    #[arg(long, value_name = "TEXT")]
    pub body: Option<String>,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    if a.title.trim().is_empty() {
        bail!("title is empty");
    }
    let issue = a.issue.as_deref().map(parse_issue).transpose()?;

    let mut repos = Vec::new();
    for p in &a.repo {
        let top = git::toplevel(p)?;
        let name = top
            .file_name()
            .and_then(|n| n.to_str())
            .context("repo path has no name")?
            .to_owned();
        let base_branch = git::default_branch(&top)?;
        repos.push(Repo {
            name,
            path: top,
            base_branch,
        });
    }
    if let Some(p) = a.parent {
        ctx.load(p).context("--parent")?;
    }
    if let Some(b) = a.blocks {
        ctx.load(b).context("--blocks")?;
    }

    let body = match a.body.as_deref() {
        Some("-") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            Some(s)
        }
        Some(t) => Some(t.to_owned()),
        None => None,
    };

    let id = counter::next_id(&ctx.store)?;
    let mut ids = vec![id];
    ids.extend(a.blocks);
    let _g = lock::lock_todos(&ctx.store, &ids)?;

    let mut t = Todo::new(id, a.title.trim(), ctx.now);
    t.meta.issue = issue;
    t.meta.repos = repos;
    t.meta.parent = a.parent;
    if let Some(b) = body {
        t.body = if b.contains("## Checklist") {
            format!("\n{}\n", b.trim_end())
        } else {
            format!("\n{}\n{}", b.trim_end(), t.body)
        };
    }

    if let Some(b) = a.blocks {
        let blocks_of = |x: u64| ctx.load(x).map(|t| t.meta.blocks).unwrap_or_default();
        if graph::would_cycle(id, b, &blocks_of) {
            bail!("#{id} blocks #{b} would create a cycle");
        }
        t.meta.blocks.push(b);
        let mut target = ctx.load(b)?;
        target.meta.blocked_by.push(id);
        target.touch(ctx.now);
        ctx.save(&target)?;
        journal::append(
            &ctx.store.journal(b),
            ctx.now,
            Kind::Status,
            crate::resolve::env_session().as_deref(),
            &format!("blocked by #{id}: {}", t.meta.title),
        )?;
    }
    ctx.save(&t)?;

    if ctx.json {
        ctx.print_json(&t.meta)
    } else {
        println!("created #{id}: {}", t.meta.title);
        Ok(())
    }
}
