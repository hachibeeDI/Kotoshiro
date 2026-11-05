use anyhow::Result;

use super::Summarizer;

/// テスト用。固定文字列を返す。
pub struct Fixed {
    text: String,
}

impl Default for Fixed {
    fn default() -> Self {
        Self::new("一行: (fixed) 作業中\nいま: fixed summarizer\n次: なし\n詰まり: なし")
    }
}

impl Fixed {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }
}

impl Summarizer for Fixed {
    fn name(&self) -> &'static str {
        "fixed"
    }

    fn summarize(&self, _prompt: &str, _input: &str) -> Result<String> {
        Ok(self.text.clone())
    }
}
