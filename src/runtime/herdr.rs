//! herdr adapter。TODO = workspace、repo = tab、セッション = pane 内の agent。
//! herdr の ID は再起動で変わりうるので保存せず、workspace は label `#<id> ` で毎回引く。

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::{Runtime, SessionState, Spawn, Workspace};
use crate::agent::{Launch, ENV_SESSION};
use crate::proc::Cmd;
use crate::store::sessions::SessionRef;

pub struct Herdr;

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// `agent start` は agent の起動完了を待つ (herdr 既定 30 秒)。
const START_TIMEOUT: Duration = Duration::from_secs(120);
const NAME_MAX: usize = 32;

// herdr の出力。フィールドは指定できないので必要な分だけ取り、未知は無視する。
#[derive(Deserialize)]
struct Envelope<T> {
    result: T,
}

#[derive(Deserialize)]
struct WorkspaceList {
    workspaces: Vec<WorkspaceInfo>,
}

#[derive(Deserialize)]
struct WorkspaceInfo {
    workspace_id: String,
    #[serde(default)]
    label: String,
}

#[derive(Deserialize)]
struct Created {
    root_pane: PaneInfo,
}

#[derive(Deserialize)]
struct PaneInfo {
    pane_id: String,
}

#[derive(Deserialize)]
struct AgentInfo {
    agent: AgentStatus,
}

#[derive(Deserialize)]
struct AgentStatus {
    #[serde(default)]
    agent_status: String,
}

fn call<T: DeserializeOwned>(args: &[String], timeout: Duration) -> Result<T> {
    let out = Cmd::new("herdr")
        .args(args.iter().cloned())
        .timeout(timeout)
        .run()
        .context("run herdr")?;
    let what = args.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    if out.code != 0 {
        bail!("herdr {what} failed: {}", error_message(&out.stderr));
    }
    let env: Envelope<T> = serde_json::from_str(out.stdout.trim())
        .with_context(|| format!("parse herdr {what} output: {}", out.stdout.trim()))?;
    Ok(env.result)
}

/// stderr は JSON (`{"error": {"message": ...}}` 系) か生テキスト。
fn error_message(stderr: &str) -> String {
    let s = stderr.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
        for path in [&["error", "message"][..], &["message"], &["error"]] {
            let mut cur = &v;
            for k in path {
                cur = &cur[k];
            }
            if let Some(m) = cur.as_str() {
                return m.to_owned();
            }
        }
    }
    if s.is_empty() {
        "(no output)".into()
    } else {
        s.to_owned()
    }
}

fn args<const N: usize>(a: [&str; N]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

pub fn label(ws: &Workspace) -> String {
    format!("#{} {}", ws.id, ws.title)
}

fn find_workspace(id: u64) -> Result<Option<String>> {
    let list: WorkspaceList = call(&args(["workspace", "list"]), CALL_TIMEOUT)?;
    let exact = format!("#{id}");
    let prefix = format!("#{id} ");
    Ok(list
        .workspaces
        .into_iter()
        .find(|w| w.label == exact || w.label.starts_with(&prefix))
        .map(|w| w.workspace_id))
}

/// herdr の agent 名規則 `[a-z][a-z0-9_-]{0,31}` に収め、session_id の先頭 4 字で一意にする。
pub fn agent_name(base: &str, session_id: &str) -> String {
    let mut s: String = base
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if !s.starts_with(|c: char| c.is_ascii_lowercase()) {
        s.insert_str(0, "t-");
    }
    let suffix: String = session_id
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(4)
        .collect();
    let keep = NAME_MAX - suffix.len() - usize::from(!suffix.is_empty());
    let s: String = s.chars().take(keep).collect();
    let s = s.trim_end_matches('-');
    if suffix.is_empty() {
        s.to_owned()
    } else {
        format!("{s}-{suffix}")
    }
}

impl Runtime for Herdr {
    fn name(&self) -> &'static str {
        "herdr"
    }

    fn spawn(&self, launch: &Launch, ws: &Workspace) -> Result<Spawn> {
        if launch.argv.first().map(String::as_str) != Some("claude") {
            bail!("herdr adapter only knows how to start `claude`");
        }
        let cwd = launch.cwd.to_string_lossy().into_owned();
        let mut envs = Vec::new();
        for (k, v) in &launch.env {
            envs.push("--env".to_owned());
            envs.push(format!("{k}={v}"));
        }
        let focus = if ws.focus { "--focus" } else { "--no-focus" };

        let pane = match find_workspace(ws.id)? {
            Some(wid) => {
                let tab_label = ws
                    .repo
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("todo-{}", ws.id));
                let mut a = args([
                    "tab",
                    "create",
                    "--workspace",
                    &wid,
                    "--cwd",
                    &cwd,
                    "--label",
                    &tab_label,
                    focus,
                ]);
                a.extend(envs);
                let c: Created = call(&a, CALL_TIMEOUT)?;
                c.root_pane.pane_id
            }
            None => {
                let mut a = args([
                    "workspace",
                    "create",
                    "--cwd",
                    &cwd,
                    "--label",
                    &label(ws),
                    focus,
                ]);
                a.extend(envs);
                let c: Created = call(&a, CALL_TIMEOUT)?;
                c.root_pane.pane_id
            }
        };

        let session_id = launch
            .env
            .iter()
            .find(|(k, _)| k == ENV_SESSION)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default();
        let base = crate::agent::session_name(ws.id, ws.repo);
        let name = agent_name(&base, session_id);
        let mut a = args([
            "agent", "start", &name, "--kind", "claude", "--pane", &pane, "--",
        ]);
        a.extend(launch.argv[1..].iter().cloned());
        let _: serde_json::Value = call(&a, START_TIMEOUT)?;
        Ok(Spawn::Started(SessionRef {
            runtime: "herdr".into(),
            reference: pane,
        }))
    }

    fn state(&self, s: &SessionRef) -> Result<SessionState> {
        if s.reference.is_empty() {
            return Ok(SessionState::Unknown);
        }
        let info: AgentInfo = call(&args(["agent", "get", &s.reference]), CALL_TIMEOUT)?;
        Ok(match info.agent.agent_status.as_str() {
            "working" => SessionState::Working,
            "blocked" => SessionState::Blocked,
            "idle" | "done" => SessionState::Idle,
            _ => SessionState::Unknown,
        })
    }

    fn close_workspace(&self, ws: &Workspace) -> Result<bool> {
        let Some(wid) = find_workspace(ws.id)? else {
            return Ok(false);
        };
        let _: serde_json::Value = call(&args(["workspace", "close", &wid]), CALL_TIMEOUT)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_name_rules() {
        let n = agent_name("todo-42-app", "3f9a1c2e-0000");
        assert_eq!(n, "todo-42-app-3f9a");
        let n = agent_name("todo-7-My.Repo", "abcdef");
        assert_eq!(n, "todo-7-my-repo-abcd");
        let long = agent_name(&format!("todo-1-{}", "x".repeat(60)), "12345678");
        assert!(long.len() <= NAME_MAX);
        assert!(long.ends_with("-1234"));
        assert_eq!(agent_name("9lives", ""), "t-9lives");
        let n = agent_name("todo-1-日本語", "ab");
        assert!(n.starts_with(|c: char| c.is_ascii_lowercase()));
        assert!(n
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'));
    }

    #[test]
    fn error_message_shapes() {
        assert_eq!(
            error_message(r#"{"error":{"message":"no such pane"}}"#),
            "no such pane"
        );
        assert_eq!(error_message(r#"{"message":"m"}"#), "m");
        assert_eq!(error_message("plain text\n"), "plain text");
        assert_eq!(error_message(""), "(no output)");
    }
}
