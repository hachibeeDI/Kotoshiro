use anyhow::Result;
use clap::Args as ClapArgs;
use serde::Serialize;

use super::Ctx;
use crate::render::{self, ShowView};
use crate::resolve;
use crate::store::digest::Digest;
use crate::store::journal::{self, Entry};
use crate::store::sessions::{self, Session};
use crate::store::todo::{Item, Meta};

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// 省略時は KOTOSHIRO_TODO
    pub id: Option<u64>,
    /// journal を末尾から何件出すか
    #[arg(long, default_value_t = 5)]
    pub journal: usize,
}

#[derive(Serialize)]
struct Out<'a> {
    #[serde(flatten)]
    meta: &'a Meta,
    blocked: bool,
    body: &'a str,
    checklist: &'a [Item],
    digest: Option<&'a Digest>,
    digest_stale: bool,
    sessions: &'a [Session],
    journal: &'a [Entry],
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let id = match a.id {
        Some(id) => id,
        None => resolve::require(&ctx.store, None)?.id,
    };
    let t = ctx.load(id)?;
    let map = ctx.status_map()?;
    let blocked = ctx.blocked(&t.meta, &map);
    let (digest, stale) = ctx.digest_state(id)?;
    let checklist = t.checklist();
    let mut sess = sessions::load(&ctx.store.sessions(id))?;
    sess.sort_by_key(|s| s.started_at);
    let recent: Vec<Session> = sess.into_iter().rev().take(5).collect();
    let tail = journal::tail(&ctx.store.journal(id), a.journal)?;

    if ctx.json {
        ctx.print_json(&Out {
            meta: &t.meta,
            blocked,
            body: &t.body,
            checklist: &checklist,
            digest: digest.as_ref(),
            digest_stale: stale,
            sessions: &recent,
            journal: &tail,
        })
    } else {
        print!(
            "{}",
            render::show(&ShowView {
                todo: &t,
                blocked,
                digest: digest.as_ref(),
                stale,
                checklist: &checklist,
                sessions: &recent,
                journal: &tail,
                now: ctx.now,
            })
        );
        Ok(())
    }
}
