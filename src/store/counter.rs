use anyhow::{Context, Result};

use super::lock;
use super::paths::Store;

/// 連番 ID を採番する。`.counter` には次に払い出す値を置く。
pub fn next_id(store: &Store) -> Result<u64> {
    store.ensure()?;
    let _g = lock::lock(&store.counter_lock())?;
    let path = store.counter_path();
    let next = match std::fs::read_to_string(&path) {
        Ok(s) => s
            .trim()
            .parse::<u64>()
            .with_context(|| format!("parse {}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 1,
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    // 手で消されたディレクトリと衝突しないよう、既存 ID の最大値も見る
    let max_existing = store.ids()?.last().copied().unwrap_or(0);
    let id = next.max(max_existing + 1);
    std::fs::write(&path, format!("{}\n", id + 1))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(id)
}
