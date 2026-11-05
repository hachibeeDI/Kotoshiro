use std::io::Read;

use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use serde::Serialize;

use super::Ctx;
use crate::resolve;
use crate::store::journal::{self, Kind};
use crate::store::lock;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// 本文。省略または `-` で stdin
    pub text: Option<String>,
    /// 対象 TODO (既定: KOTOSHIRO_TODO)
    #[arg(long, value_name = "ID")]
    pub todo: Option<u64>,
}

#[derive(Serialize)]
struct Out {
    id: u64,
    session_id: Option<String>,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let text = match a.text.as_deref() {
        None | Some("-") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        }
        Some(t) => t.to_owned(),
    };
    if text.trim().is_empty() {
        bail!("note is empty");
    }
    let cur = resolve::require(&ctx.store, a.todo)?;
    let _g = lock::lock_todo(&ctx.store, cur.id)?;
    let mut t = ctx.load(cur.id)?;
    journal::append(
        &ctx.store.journal(cur.id),
        ctx.now,
        Kind::Note,
        cur.session_id.as_deref(),
        &text,
    )?;
    t.touch(ctx.now);
    ctx.save(&t)?;
    if ctx.json {
        ctx.print_json(&Out {
            id: cur.id,
            session_id: cur.session_id,
        })
    } else {
        println!("noted #{}", cur.id);
        Ok(())
    }
}
