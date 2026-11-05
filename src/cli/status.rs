use anyhow::{bail, Result};
use clap::Args as ClapArgs;

use super::Ctx;
use crate::store::journal::{self, Kind};
use crate::store::lock;
use crate::store::todo::Status;

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub id: u64,
    #[arg(value_enum)]
    pub status: Status,
    /// done の条件 (checklist 全消化、blocked_by 全 done) を無視する
    #[arg(long)]
    pub force: bool,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let _g = lock::lock_todo(&ctx.store, a.id)?;
    let mut t = ctx.load(a.id)?;
    if a.status == Status::Done && !a.force {
        let mut reasons = Vec::new();
        let left = t.checklist().iter().filter(|i| !i.done).count();
        if left > 0 {
            reasons.push(format!("{left} checklist item(s) left"));
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
            reasons.push(format!("blocked by {}", open.join(", ")));
        }
        if !reasons.is_empty() {
            bail!(
                "cannot mark #{} done: {} (use --force)",
                a.id,
                reasons.join("; ")
            );
        }
    }
    let old = t.meta.status;
    t.meta.status = a.status;
    t.touch(ctx.now);
    ctx.save(&t)?;
    journal::append(
        &ctx.store.journal(a.id),
        ctx.now,
        Kind::Status,
        crate::resolve::env_session().as_deref(),
        &format!("{old} -> {}", a.status),
    )?;
    if ctx.json {
        ctx.print_json(&t.meta)
    } else {
        println!("#{}: {old} -> {}", a.id, a.status);
        Ok(())
    }
}
