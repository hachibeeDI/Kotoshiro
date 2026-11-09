pub mod claude;
pub mod fixed;

use anyhow::Result;

use crate::config::Config;

pub trait Summarizer {
    fn name(&self) -> &'static str;
    /// `prompt` は指示、`input` は要約対象 (stdin 相当)。
    fn summarize(&self, prompt: &str, input: &str) -> Result<String>;
}

/// `KOTOSHIRO_SUMMARIZER`: `claude` (既定) | `fixed` | `fixed:<text>`
pub fn select(config: &Config) -> Box<dyn Summarizer> {
    match std::env::var("KOTOSHIRO_SUMMARIZER").ok().as_deref() {
        Some("fixed") => Box::new(fixed::Fixed::default()),
        Some(s) if s.starts_with("fixed:") => Box::new(fixed::Fixed::new(&s["fixed:".len()..])),
        _ => Box::new(claude::Claude::new(config.claude_config_dir())),
    }
}
