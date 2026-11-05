use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};

use super::paths::Store;

/// 排他ロック。drop でファイルが閉じ、OS がロックを解放する。
pub struct Guard(#[allow(dead_code)] File);

pub fn lock(path: &Path) -> Result<Guard> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let f = File::options()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("open lock {}", path.display()))?;
    f.lock()
        .with_context(|| format!("lock {}", path.display()))?;
    Ok(Guard(f))
}

pub fn lock_todo(store: &Store, id: u64) -> Result<Guard> {
    lock(&store.lock_path(id))
}

/// 複数 TODO を触る時は ID 昇順に取る (デッドロック回避)。
pub fn lock_todos(store: &Store, ids: &[u64]) -> Result<Vec<Guard>> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    ids.iter().map(|&id| lock_todo(store, id)).collect()
}
