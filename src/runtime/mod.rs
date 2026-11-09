pub mod herdr;
pub mod manual;

use anyhow::{bail, Result};

use crate::agent::Launch;
use crate::store::sessions::SessionRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Working,
    Blocked,
    Idle,
    Unknown,
}

impl SessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionState::Working => "working",
            SessionState::Blocked => "blocked",
            SessionState::Idle => "idle",
            SessionState::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// adapter が workspace を作る / 探す時に使う TODO 側の情報。
pub struct Workspace<'a> {
    pub id: u64,
    pub title: &'a str,
    pub repo: Option<&'a str>,
    pub focus: bool,
}

pub enum Spawn {
    /// 呼び出し元プロセスが claude に差し替わる (manual)。
    Exec,
    /// adapter が起動済み。
    Started(SessionRef),
}

/// 周辺ツール (herdr / tmux) への port。
pub trait Runtime {
    fn name(&self) -> &'static str;
    fn spawn(&self, launch: &Launch, ws: &Workspace) -> Result<Spawn>;
    fn state(&self, s: &SessionRef) -> Result<SessionState>;
    /// TODO の workspace を閉じる。無ければ false。
    fn close_workspace(&self, ws: &Workspace) -> Result<bool>;
}

pub const ENV_RUNTIME: &str = "KOTOSHIRO_RUNTIME";

pub fn by_name(name: &str) -> Option<Box<dyn Runtime>> {
    match name {
        "manual" => Some(Box::new(manual::Manual)),
        "herdr" => Some(Box::new(herdr::Herdr)),
        _ => None,
    }
}

/// 名前は `Config::runtime` で決める (フラグ > env > config > manual)。自動検出はしない。
pub fn select(name: &str) -> Result<Box<dyn Runtime>> {
    match by_name(name) {
        Some(r) => Ok(r),
        None => bail!("runtime `{name}` is not implemented (available: manual, herdr)"),
    }
}
