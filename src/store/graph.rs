use std::collections::HashSet;

use super::todo::Status;

/// `from` が `to` を block する辺を足した時に閉路ができるか。
/// `blocks_of(id)` は id が block している TODO 群を返す。
pub fn would_cycle(from: u64, to: u64, blocks_of: &dyn Fn(u64) -> Vec<u64>) -> bool {
    if from == to {
        return true;
    }
    // to から辿って from に戻れれば閉路
    let mut seen = HashSet::new();
    let mut stack = vec![to];
    while let Some(cur) = stack.pop() {
        if cur == from {
            return true;
        }
        if !seen.insert(cur) {
            continue;
        }
        stack.extend(blocks_of(cur));
    }
    false
}

/// blocked は導出値: blocked_by に未完了 TODO があれば true。
pub fn is_blocked(blocked_by: &[u64], status_of: &dyn Fn(u64) -> Option<Status>) -> bool {
    blocked_by
        .iter()
        .any(|&id| !matches!(status_of(id), Some(Status::Done)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_detection() {
        // 1 blocks 2, 2 blocks 3
        let blocks = |id: u64| match id {
            1 => vec![2],
            2 => vec![3],
            _ => vec![],
        };
        assert!(would_cycle(3, 1, &blocks));
        assert!(would_cycle(2, 1, &blocks));
        assert!(!would_cycle(1, 3, &blocks));
        assert!(!would_cycle(4, 1, &blocks));
        assert!(would_cycle(5, 5, &blocks));
    }

    #[test]
    fn blocked_derivation() {
        let st = |id: u64| match id {
            1 => Some(Status::Done),
            2 => Some(Status::Active),
            _ => None,
        };
        assert!(!is_blocked(&[], &st));
        assert!(!is_blocked(&[1], &st));
        assert!(is_blocked(&[1, 2], &st));
        // 消された TODO は未完了扱い (安全側)
        assert!(is_blocked(&[9], &st));
    }
}
