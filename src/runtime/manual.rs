use anyhow::Result;

use super::{Runtime, Spawned};
use crate::agent::Launch;
use crate::store::sessions::SessionRef;
use crate::util::shell_quote;

/// 実行すべきコマンドを組み立てて返すだけ。起動は人 (または `start --exec`)。
pub struct Manual;

impl Runtime for Manual {
    fn name(&self) -> &'static str {
        "manual"
    }

    fn spawn(&self, launch: &Launch) -> Result<Spawned> {
        let mut parts = vec![
            format!("cd {}", shell_quote(&launch.cwd.to_string_lossy())),
            "&&".into(),
        ];
        for (k, v) in &launch.env {
            parts.push(format!("{k}={}", shell_quote(v)));
        }
        parts.extend(launch.argv.iter().map(|a| shell_quote(a)));
        Ok(Spawned {
            reference: SessionRef {
                runtime: "manual".into(),
                reference: String::new(),
            },
            shell_command: Some(parts.join(" ")),
        })
    }
}
