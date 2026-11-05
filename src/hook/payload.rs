use std::path::PathBuf;

use serde::Deserialize;

// Claude Code 由来の payload はフィールドがバージョン毎に増える。
// 必要なものだけ取り、未知フィールドは無視する (DESIGN.md 実装方針)。

#[derive(Debug, Deserialize)]
pub struct Stop {
    pub session_id: String,
    pub cwd: PathBuf,
    #[serde(default)]
    pub last_assistant_message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionEnd {
    pub session_id: String,
    pub cwd: PathBuf,
    #[serde(default)]
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_ignored() {
        let j = r#"{"session_id":"s1","cwd":"/tmp","hook_event_name":"Stop","stop_hook_active":false,
                    "last_assistant_message":"done","background_tasks":[],"session_crons":[],"effort":"high"}"#;
        let p: Stop = serde_json::from_str(j).unwrap();
        assert_eq!(p.session_id, "s1");
        assert_eq!(p.last_assistant_message.as_deref(), Some("done"));
        let e: SessionEnd =
            serde_json::from_str(r#"{"session_id":"s1","cwd":"/tmp","reason":"other","x":1}"#)
                .unwrap();
        assert_eq!(e.reason.as_deref(), Some("other"));
    }
}
