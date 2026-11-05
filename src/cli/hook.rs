//! Claude Code hook のエントリ。stdin に JSON、常に exit 0、Claude を block しない。

use std::fs::File;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use clap::Subcommand;

use super::Ctx;
use crate::agent::ENV_NESTED;
use crate::hook::payload;
use crate::resolve::{self, Current};
use crate::store::journal::{self, Kind};
use crate::store::lock;
use crate::store::paths::file_bytes;
use crate::store::sessions::{self, Session, SessionRef};

#[derive(Subcommand, Debug)]
pub enum Event {
    /// Stop hook: last_assistant_message を log/ に追記
    Stop,
    /// SessionEnd hook: ended_at を記録し digest を detach 起動
    SessionEnd,
}

pub fn run(ctx: &Ctx, ev: Event) -> Result<()> {
    if std::env::var_os(ENV_NESTED).is_some() {
        return Ok(());
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let r = match ev {
        Event::Stop => stop(ctx, &input),
        Event::SessionEnd => session_end(ctx, &input),
    };
    if let Err(e) = r {
        eprintln!("kotoshiro hook: {e:#}");
    }
    Ok(())
}

fn stop(ctx: &Ctx, input: &str) -> Result<()> {
    let p: payload::Stop = serde_json::from_str(input).context("parse Stop payload")?;
    let Some(cur) = resolve::resolve(&ctx.store, None, Some(&p.session_id), Some(&p.cwd))? else {
        return Ok(());
    };
    let Some(msg) = p
        .last_assistant_message
        .as_deref()
        .filter(|m| !m.trim().is_empty())
    else {
        return Ok(());
    };
    let _g = lock::lock_todo(&ctx.store, cur.id)?;
    let log_dir = ctx.store.log_dir(cur.id);
    std::fs::create_dir_all(&log_dir)?;
    let log = ctx.store.log_file(cur.id, &p.session_id);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;
    write!(f, "## {}\n\n{}\n\n", ctx.now, msg.trim_end())?;
    drop(f);

    let spath = ctx.store.sessions(cur.id);
    let mut list = sessions::load(&spath)?;
    match list.iter_mut().find(|s| s.session_id == p.session_id) {
        Some(s) => s.log_bytes = file_bytes(&log),
        None => list.push(Session {
            // 手で起動されたセッション (env / cwd で紐付いた) をここで登録する
            session_id: p.session_id.clone(),
            runtime: SessionRef {
                runtime: "manual".into(),
                reference: String::new(),
            },
            repo: None,
            worktree: None,
            cwd: p.cwd.clone(),
            started_at: ctx.now,
            ended_at: None,
            log_bytes: file_bytes(&log),
        }),
    }
    sessions::save(&spath, &list)
}

fn session_end(ctx: &Ctx, input: &str) -> Result<()> {
    let p: payload::SessionEnd = serde_json::from_str(input).context("parse SessionEnd payload")?;
    let Some(cur) = resolve::resolve(&ctx.store, None, Some(&p.session_id), Some(&p.cwd))? else {
        return Ok(());
    };
    {
        let _g = lock::lock_todo(&ctx.store, cur.id)?;
        let spath = ctx.store.sessions(cur.id);
        let mut list = sessions::load(&spath)?;
        if let Some(s) = list.iter_mut().find(|s| s.session_id == p.session_id) {
            s.ended_at = Some(ctx.now);
            sessions::save(&spath, &list)?;
        }
        journal::append(
            &ctx.store.journal(cur.id),
            ctx.now,
            Kind::End,
            Some(&p.session_id),
            &format!(
                "session ended ({})",
                p.reason.as_deref().unwrap_or("unknown")
            ),
        )?;
    }
    spawn_digest(ctx, &cur)
}

/// Claude の終了をブロックしないよう、別プロセスグループで digest を起動して待たない。
fn spawn_digest(ctx: &Ctx, cur: &Current) -> Result<()> {
    let exe = std::env::current_exe().context("current_exe")?;
    let log = File::create(ctx.store.digest_log(cur.id))?;
    let mut c = Command::new(exe);
    c.args(["digest", &cur.id.to_string(), "--quiet"])
        .env("KOTOSHIRO_HOME", ctx.store.root())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    c.spawn().context("spawn digest")?;
    Ok(())
}
