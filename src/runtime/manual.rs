use anyhow::Result;

use super::{Runtime, SessionState, Spawn, Workspace};
use crate::agent::Launch;
use crate::store::sessions::SessionRef;
use crate::util::shell_quote;

/// 起動は呼び出し元プロセスの exec (または人が `--print` の出力を打つ)。
pub struct Manual;

/// `cd <cwd> && K=V ... claude ...` の形。どの adapter でも `--print` はこれを出す。
pub fn shell_command(launch: &Launch) -> String {
    let mut parts = vec![
        format!("cd {}", shell_quote(&launch.cwd.to_string_lossy())),
        "&&".into(),
    ];
    for (k, v) in &launch.env {
        parts.push(format!("{k}={}", shell_quote(v)));
    }
    parts.extend(launch.argv.iter().map(|a| shell_quote(a)));
    parts.join(" ")
}

impl Runtime for Manual {
    fn name(&self) -> &'static str {
        "manual"
    }

    fn spawn(&self, _launch: &Launch, _ws: &Workspace) -> Result<Spawn> {
        Ok(Spawn::Exec)
    }

    fn state(&self, _s: &SessionRef) -> Result<SessionState> {
        Ok(SessionState::Unknown)
    }

    fn close_workspace(&self, _ws: &Workspace) -> Result<bool> {
        Ok(false)
    }
}
