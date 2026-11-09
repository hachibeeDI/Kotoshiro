//! `$KOTOSHIRO_HOME/config.toml`。マシン単位の好みだけを持つ。
//! 優先順はコマンドのフラグ > 環境変数 > この file > 既定。

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::runtime::ENV_RUNTIME;
use crate::store::todo::Repo;
use crate::store::Store;

pub const FILE: &str = "config.toml";
pub const ENV_CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
pub const DEFAULT_WORKTREE: &str = "{repo}/../{name}-wt/todo-{id}";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// manual | herdr
    pub runtime: Option<String>,
    /// worktree の置き場。`{repo}` `{name}` `{id}` を展開する。相対ならリポ基準
    pub worktree: Option<String>,
    /// Claude Code の設定ディレクトリ。init の書き先、start / 要約器の子プロセスの CLAUDE_CONFIG_DIR
    pub claude_config_dir: Option<PathBuf>,
}

impl Config {
    pub fn path(store: &Store) -> PathBuf {
        store.root().join(FILE)
    }

    pub fn load(store: &Store) -> Result<Self> {
        let p = Self::path(store);
        match std::fs::read_to_string(&p) {
            Ok(s) => Self::parse(&s).with_context(|| format!("parse {}", p.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("read {}", p.display())),
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(toml_edit::de::from_str(s)?)
    }

    /// フラグ > `KOTOSHIRO_RUNTIME` > config > manual
    pub fn runtime(&self, flag: Option<&str>) -> String {
        flag.map(str::to_owned)
            .or_else(|| env_nonempty(ENV_RUNTIME))
            .or_else(|| self.runtime.clone())
            .unwrap_or_else(|| "manual".into())
    }

    pub fn worktree_dir(&self, repo: &Repo, id: u64) -> PathBuf {
        expand_worktree(
            self.worktree.as_deref().unwrap_or(DEFAULT_WORKTREE),
            repo,
            id,
        )
    }

    /// `CLAUDE_CONFIG_DIR` > config。None なら claude の既定 (`~/.claude`) に任せる
    pub fn claude_config_dir(&self) -> Option<PathBuf> {
        env_nonempty(ENV_CLAUDE_CONFIG_DIR)
            .map(PathBuf::from)
            .or_else(|| self.claude_config_dir.as_deref().map(expand_tilde))
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

pub fn expand_worktree(template: &str, repo: &Repo, id: u64) -> PathBuf {
    let s = template
        .replace("{repo}", &repo.path.to_string_lossy())
        .replace("{name}", &repo.name)
        .replace("{id}", &id.to_string());
    let p = expand_tilde(Path::new(&s));
    let p = if p.is_absolute() {
        p
    } else {
        repo.path.join(p)
    };
    normalize(&p)
}

pub fn expand_tilde(p: &Path) -> PathBuf {
    let Some(home) = dirs::home_dir() else {
        return p.to_path_buf();
    };
    match p.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => p.to_path_buf(),
    }
}

/// `..` と `.` を字句的に畳む (symlink は見ない)。
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// init が書く雛形。`claude_config_dir` は明示された時だけ有効行にする。
pub fn template(claude_config_dir: Option<&Path>) -> String {
    let cc = match claude_config_dir {
        Some(p) => format!("claude_config_dir = {}", toml_str(&p.to_string_lossy())),
        None => "# claude_config_dir = \"~/.claude\"".into(),
    };
    format!(
        "# kotoshiro の設定。優先順: コマンドのフラグ > 環境変数 > この file > 既定\n\
         \n\
         # セッションを起動する adapter: manual | herdr  (env: KOTOSHIRO_RUNTIME)\n\
         # runtime = \"manual\"\n\
         \n\
         # worktree の置き場。{{repo}} = リポの絶対パス、{{name}} = ディレクトリ名、{{id}} = TODO ID。相対ならリポ基準\n\
         # worktree = \"{DEFAULT_WORKTREE}\"\n\
         \n\
         # Claude Code の設定ディレクトリ。init の書き先になり、start が起動する claude と要約器にも\n\
         # CLAUDE_CONFIG_DIR として渡す  (env: CLAUDE_CONFIG_DIR)\n\
         {cc}\n"
    )
}

/// 既存の config.toml の `claude_config_dir` を書き換える (コメントは保つ)。変更があれば true。
pub fn set_claude_config_dir(path: &Path, dir: &Path) -> Result<bool> {
    let s = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = s
        .parse()
        .with_context(|| format!("parse {}", path.display()))?;
    let v = dir.to_string_lossy().into_owned();
    if doc
        .get("claude_config_dir")
        .and_then(|i| i.as_str())
        .is_some_and(|cur| cur == v)
    {
        return Ok(false);
    }
    doc["claude_config_dir"] = toml_edit::value(v);
    std::fs::write(path, doc.to_string()).with_context(|| format!("write {}", path.display()))?;
    Ok(true)
}

fn toml_str(s: &str) -> String {
    toml_edit::Value::from(s).to_string().trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> Repo {
        Repo {
            name: "app".into(),
            path: PathBuf::from("/src/app"),
            base_branch: "main".into(),
        }
    }

    #[test]
    fn worktree_templates() {
        let r = repo();
        assert_eq!(
            expand_worktree(DEFAULT_WORKTREE, &r, 42),
            PathBuf::from("/src/app-wt/todo-42")
        );
        assert_eq!(
            expand_worktree("{repo}/.claude/worktrees/todo-{id}", &r, 7),
            PathBuf::from("/src/app/.claude/worktrees/todo-7")
        );
        // 相対はリポ基準
        assert_eq!(
            expand_worktree(".wt/{name}-{id}", &r, 1),
            PathBuf::from("/src/app/.wt/app-1")
        );
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            expand_worktree("~/wt/{id}", &r, 1),
            home.join("wt").join("1")
        );
    }

    #[test]
    fn parse_and_defaults() {
        let c = Config::parse("runtime = \"herdr\"\nworktree = \"x/{id}\"\n").unwrap();
        assert_eq!(c.runtime.as_deref(), Some("herdr"));
        assert_eq!(c.worktree_dir(&repo(), 3), PathBuf::from("/src/app/x/3"));
        assert!(Config::parse("runtim = 1").is_err());
        let d = Config::parse(&template(None)).unwrap();
        assert!(d.runtime.is_none() && d.claude_config_dir.is_none());
        let t = Config::parse(&template(Some(Path::new("/c/d")))).unwrap();
        assert_eq!(t.claude_config_dir, Some(PathBuf::from("/c/d")));
    }

    #[test]
    fn set_dir_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, template(None)).unwrap();
        assert!(set_claude_config_dir(&p, Path::new("/x/.claude-work")).unwrap());
        assert!(!set_claude_config_dir(&p, Path::new("/x/.claude-work")).unwrap());
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("# runtime = \"manual\""));
        assert_eq!(
            Config::parse(&s).unwrap().claude_config_dir,
            Some(PathBuf::from("/x/.claude-work"))
        );
    }
}
