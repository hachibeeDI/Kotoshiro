pub mod manual;

use anyhow::{bail, Result};

use crate::agent::Launch;
use crate::store::sessions::SessionRef;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Working,
    Blocked,
    Idle,
    Exited,
    Unknown,
}

pub struct Spawned {
    pub reference: SessionRef,
    /// manual adapter が返す「ユーザーが打つべきコマンド」。
    pub shell_command: Option<String>,
}

/// 周辺ツール (herdr / tmux) への port。週 1 は manual のみ。
pub trait Runtime {
    fn name(&self) -> &'static str;
    fn spawn(&self, launch: &Launch) -> Result<Spawned>;
}

/// `KOTOSHIRO_RUNTIME` (既定 manual)。自動検出はしない。
pub fn from_env() -> Result<Box<dyn Runtime>> {
    match std::env::var("KOTOSHIRO_RUNTIME").ok().as_deref() {
        None | Some("manual") => Ok(Box::new(manual::Manual)),
        Some(other) => bail!("runtime `{other}` is not implemented (available: manual)"),
    }
}
