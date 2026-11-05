use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// ストアのルート (`$KOTOSHIRO_HOME`、既定 `~/todos`) とその配下のパス規則。
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn from_env() -> Result<Self> {
        let root = match std::env::var_os("KOTOSHIRO_HOME") {
            Some(p) => PathBuf::from(p),
            None => dirs::home_dir()
                .context("home directory not found")?
                .join("todos"),
        };
        Ok(Self { root })
    }

    #[allow(dead_code)]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("create {}", self.root.display()))
    }

    pub fn counter_path(&self) -> PathBuf {
        self.root.join(".counter")
    }

    pub fn counter_lock(&self) -> PathBuf {
        self.root.join(".counter.lock")
    }

    pub fn todo_dir(&self, id: u64) -> PathBuf {
        self.root.join(id.to_string())
    }

    pub fn lock_path(&self, id: u64) -> PathBuf {
        self.todo_dir(id).join(".lock")
    }

    pub fn todo_md(&self, id: u64) -> PathBuf {
        self.todo_dir(id).join("todo.md")
    }

    pub fn journal(&self, id: u64) -> PathBuf {
        self.todo_dir(id).join("journal.md")
    }

    pub fn digest(&self, id: u64) -> PathBuf {
        self.todo_dir(id).join("digest.md")
    }

    pub fn digest_log(&self, id: u64) -> PathBuf {
        self.todo_dir(id).join("digest.log")
    }

    pub fn sessions(&self, id: u64) -> PathBuf {
        self.todo_dir(id).join("sessions.json")
    }

    pub fn context(&self, id: u64) -> PathBuf {
        self.todo_dir(id).join("context.md")
    }

    pub fn log_dir(&self, id: u64) -> PathBuf {
        self.todo_dir(id).join("log")
    }

    pub fn log_file(&self, id: u64, session_id: &str) -> PathBuf {
        self.log_dir(id).join(format!("{session_id}.md"))
    }

    pub fn exists(&self, id: u64) -> bool {
        self.todo_md(id).is_file()
    }

    /// 存在する TODO の ID を昇順で返す。
    pub fn ids(&self) -> Result<Vec<u64>> {
        let mut ids = Vec::new();
        let rd = match std::fs::read_dir(&self.root) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
            Err(e) => return Err(e).with_context(|| format!("read {}", self.root.display())),
        };
        for ent in rd {
            let ent = ent?;
            let Some(name) = ent.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Ok(id) = name.parse::<u64>() {
                if self.exists(id) {
                    ids.push(id);
                }
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }
}

/// log ディレクトリ全体のバイト数。digest の stale 判定に使う。
pub fn dir_bytes(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

pub fn file_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}
