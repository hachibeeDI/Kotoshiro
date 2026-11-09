use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use super::Summarizer;
use crate::proc::Cmd;

/// `claude -p --model haiku` を子プロセスで叩く。ユーザーの OAuth ログインを流用する。
/// `--bare` は OAuth を読まない仕様なので使えない。hook の再帰は `KOTOSHIRO_NESTED=1` で止める
/// (hook コマンドはこれを見たら即終了)。`--setting-sources ""` と `--strict-mcp-config` で
/// 設定・MCP の読み込みを省き、起動とトークンを削る。
pub struct Claude {
    pub model: String,
    pub timeout: Duration,
    /// 子プロセスに渡す CLAUDE_CONFIG_DIR。ログイン情報もここにあるので env の無い所から呼ぶ時に効く
    pub config_dir: Option<PathBuf>,
}

impl Claude {
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        Self {
            model: std::env::var("KOTOSHIRO_SUMMARIZER_MODEL").unwrap_or_else(|_| "haiku".into()),
            timeout: Duration::from_secs(60),
            config_dir,
        }
    }
}

// `claude -p --output-format json` の出力。必要フィールドのみ。
#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    result: String,
    #[serde(default)]
    is_error: bool,
}

impl Summarizer for Claude {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn summarize(&self, prompt: &str, input: &str) -> Result<String> {
        let mut cmd = Cmd::new("claude")
            .args(["-p", prompt])
            .args(["--model", &self.model])
            .args(["--output-format", "json", "--no-session-persistence"])
            .args(["--setting-sources", "", "--strict-mcp-config"])
            .env("KOTOSHIRO_NESTED", "1")
            .stdin(input)
            .timeout(self.timeout);
        if let Some(d) = &self.config_dir {
            cmd = cmd.env(
                crate::config::ENV_CLAUDE_CONFIG_DIR,
                d.to_string_lossy().into_owned(),
            );
        }
        let out = cmd.run().context("run claude")?;
        if out.code != 0 {
            bail!("claude exited {}: {}", out.code, out.stderr.trim());
        }
        let reply: Reply = serde_json::from_str(out.stdout.trim())
            .with_context(|| format!("parse claude output: {}", out.stdout.trim()))?;
        if reply.is_error {
            bail!("claude returned error: {}", reply.result);
        }
        Ok(reply.result)
    }
}
