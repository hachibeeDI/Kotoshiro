use anyhow::Result;
use clap::Args as ClapArgs;
use jiff::Timestamp;
use serde::Serialize;

use super::Ctx;
use crate::render::{self, Row};
use crate::store::sessions;
use crate::store::todo::Status;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// done も含める
    #[arg(long)]
    pub all: bool,
}

#[derive(Serialize)]
struct Item {
    id: u64,
    title: String,
    status: Status,
    blocked: bool,
    digest: Option<String>,
    stale: bool,
    updated_at: Timestamp,
    live_sessions: usize,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let map = ctx.status_map()?;
    let mut rows = Vec::new();
    for id in ctx.store.ids()? {
        let t = match ctx.load(id) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("warning: skip #{id}: {e:#}");
                continue;
            }
        };
        if !a.all && t.meta.status == Status::Done {
            continue;
        }
        let (d, stale) = ctx.digest_state(id)?;
        let live = sessions::live_count(&sessions::load(&ctx.store.sessions(id))?);
        rows.push(Row {
            id,
            status: t.meta.status,
            blocked: ctx.blocked(&t.meta, &map),
            title: t.meta.title.clone(),
            line: d.map(|d| d.line),
            stale,
            updated_at: t.meta.updated_at,
            live,
        });
    }
    if ctx.json {
        let items: Vec<Item> = rows
            .iter()
            .map(|r| Item {
                id: r.id,
                title: r.title.clone(),
                status: r.status,
                blocked: r.blocked,
                digest: r.line.clone(),
                stale: r.stale,
                updated_at: r.updated_at,
                live_sessions: r.live,
            })
            .collect();
        ctx.print_json(&items)
    } else {
        print!("{}", render::list(&rows, ctx.now));
        Ok(())
    }
}
