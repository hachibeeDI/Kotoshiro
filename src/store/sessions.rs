use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// Runtime adapter 固有の不透明参照。manual は ref 空。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRef {
    pub runtime: String,
    #[serde(rename = "ref")]
    pub reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub runtime: SessionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<PathBuf>,
    pub cwd: PathBuf,
    pub started_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
    #[serde(default)]
    pub log_bytes: u64,
}

impl Session {
    pub fn is_live(&self) -> bool {
        self.ended_at.is_none()
    }
}

pub fn load(path: &Path) -> Result<Vec<Session>> {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).with_context(|| format!("parse {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

pub fn save(path: &Path, sessions: &[Session]) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let mut s = serde_json::to_string_pretty(sessions)?;
    s.push('\n');
    std::fs::write(&tmp, s)?;
    std::fs::rename(&tmp, path).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn live_count(sessions: &[Session]) -> usize {
    sessions.iter().filter(|s| s.is_live()).count()
}
