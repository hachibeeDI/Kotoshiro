//! Claude Code の起動コマンド組み立て。claude への依存はここに集める。

use std::path::{Path, PathBuf};

pub const ENV_TODO: &str = "KOTOSHIRO_TODO";
pub const ENV_SESSION: &str = "KOTOSHIRO_SESSION";
pub const ENV_NESTED: &str = "KOTOSHIRO_NESTED";

#[derive(Debug, Clone)]
pub struct Launch {
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub argv: Vec<String>,
}

pub fn claude_launch(
    cwd: &Path,
    todo_id: u64,
    session_id: &str,
    name: &str,
    context_file: &Path,
) -> Launch {
    Launch {
        cwd: cwd.to_path_buf(),
        env: vec![
            (ENV_TODO.into(), todo_id.to_string()),
            (ENV_SESSION.into(), session_id.to_owned()),
        ],
        argv: vec![
            "claude".into(),
            "--session-id".into(),
            session_id.to_owned(),
            "--name".into(),
            name.to_owned(),
            "--append-system-prompt-file".into(),
            context_file.to_string_lossy().into_owned(),
        ],
    }
}

pub fn session_name(todo_id: u64, repo: Option<&str>) -> String {
    match repo {
        Some(r) => format!("todo-{todo_id}-{r}"),
        None => format!("todo-{todo_id}"),
    }
}
