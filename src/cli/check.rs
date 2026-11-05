use anyhow::Result;
use clap::Args as ClapArgs;

use super::Ctx;
use crate::resolve;
use crate::store::journal::{self, Kind};
use crate::store::lock;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// checklist の番号 (1 始まり、`show` の表示順)
    pub n: usize,
    /// チェックを外す
    #[arg(long)]
    pub undo: bool,
    /// 対象 TODO (既定: KOTOSHIRO_TODO)
    #[arg(long, value_name = "ID")]
    pub todo: Option<u64>,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let cur = resolve::require(&ctx.store, a.todo)?;
    let _g = lock::lock_todo(&ctx.store, cur.id)?;
    let mut t = ctx.load(cur.id)?;
    let item = t.set_checked(a.n, !a.undo)?;
    t.touch(ctx.now);
    ctx.save(&t)?;
    let mark = if item.done { 'x' } else { ' ' };
    journal::append(
        &ctx.store.journal(cur.id),
        ctx.now,
        Kind::Check,
        cur.session_id.as_deref(),
        &format!("[{mark}] {}", item.text),
    )?;
    if ctx.json {
        ctx.print_json(&serde_json::json!({ "id": cur.id, "n": a.n, "item": item }))
    } else {
        println!("#{} [{mark}] {}", cur.id, item.text);
        Ok(())
    }
}
