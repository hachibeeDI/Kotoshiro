//! 「いまどの TODO の作業か」の解決。優先順は DESIGN.md「セッションと TODO の紐付け」。

use std::path::Path;

use anyhow::{Context, Result};

use crate::agent::{ENV_SESSION, ENV_TODO};
use crate::store::{sessions, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    pub id: u64,
    pub session_id: Option<String>,
}

pub fn env_todo() -> Result<Option<u64>> {
    match std::env::var(ENV_TODO) {
        Ok(s) if !s.trim().is_empty() => {
            Ok(Some(s.trim().parse().with_context(|| {
                format!("{ENV_TODO}={s} is not a TODO id")
            })?))
        }
        _ => Ok(None),
    }
}

pub fn env_session() -> Option<String> {
    std::env::var(ENV_SESSION).ok().filter(|s| !s.is_empty())
}

/// 1. 明示 (`--todo`) 2. env 3. session_id 照合 4. cwd 照合
pub fn resolve(
    store: &Store,
    explicit: Option<u64>,
    session_id: Option<&str>,
    cwd: Option<&Path>,
) -> Result<Option<Current>> {
    let sid = session_id.map(str::to_owned).or_else(env_session);
    if let Some(id) = explicit.or(env_todo()?) {
        return Ok(Some(Current {
            id,
            session_id: sid,
        }));
    }
    let ids = store.ids()?;
    if let Some(s) = sid.as_deref() {
        for &id in &ids {
            let list = sessions::load(&store.sessions(id))?;
            if list.iter().any(|x| x.session_id == s) {
                return Ok(Some(Current {
                    id,
                    session_id: Some(s.to_owned()),
                }));
            }
        }
    }
    if let Some(cwd) = cwd {
        let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        for &id in &ids {
            let list = sessions::load(&store.sessions(id))?;
            // 同じ worktree に複数セッションがあれば最新の live なものを優先
            let hit = list
                .iter()
                .rev()
                .filter(|x| x.worktree.as_deref().is_some_and(|w| cwd.starts_with(w)))
                .max_by_key(|x| (x.is_live(), x.started_at));
            if let Some(h) = hit {
                return Ok(Some(Current {
                    id,
                    session_id: Some(h.session_id.clone()),
                }));
            }
        }
    }
    Ok(None)
}

/// 該当が無ければエラー (note / check 用)。
pub fn require(store: &Store, explicit: Option<u64>) -> Result<Current> {
    resolve(store, explicit, None, None)?.context(format!(
        "no current TODO: pass --todo <id> or set {ENV_TODO}"
    ))
}
