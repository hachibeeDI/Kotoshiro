use std::path::Path;

use anyhow::{bail, Context, Result};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

pub const LINE_MAX_CHARS: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Head {
    pub updated_at: Timestamp,
    pub journal_bytes: u64,
    pub log_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Digest {
    #[serde(flatten)]
    pub head: Head,
    pub line: String,
    pub now: String,
    pub next: String,
    pub stuck: String,
}

/// LLM に出させる固定書式のラベル。
pub const LABELS: [&str; 4] = ["一行:", "いま:", "次:", "詰まり:"];
const HEADINGS: [&str; 3] = ["## いま", "## 次", "## 詰まり"];

impl Digest {
    pub fn is_stale(&self, journal_bytes: u64, log_bytes: u64) -> bool {
        journal_bytes > self.head.journal_bytes || log_bytes > self.head.log_bytes
    }

    pub fn load(path: &Path) -> Result<Option<Self>> {
        let s = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
        };
        Self::parse(&s)
            .map(Some)
            .with_context(|| format!("parse {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("md.tmp");
        std::fs::write(&tmp, self.render()?)?;
        std::fs::rename(&tmp, path).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub fn parse(s: &str) -> Result<Self> {
        let rest = s.strip_prefix("---\n").context("missing frontmatter")?;
        let (front, body) = rest
            .split_once("\n---\n")
            .context("unterminated frontmatter")?;
        let head: Head = serde_yaml::from_str(front).context("frontmatter")?;
        let (line, sections) = split_sections(body);
        let get = |h: &str| sections.get(h).cloned().unwrap_or_default();
        Ok(Self {
            head,
            line,
            now: get(HEADINGS[0]),
            next: get(HEADINGS[1]),
            stuck: get(HEADINGS[2]),
        })
    }

    pub fn render(&self) -> Result<String> {
        let front = serde_yaml::to_string(&self.head)?;
        Ok(format!(
            "---\n{front}---\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}\n",
            self.line,
            HEADINGS[0],
            self.now.trim(),
            HEADINGS[1],
            self.next.trim(),
            HEADINGS[2],
            self.stuck.trim(),
        ))
    }

    /// LLM 出力 (`一行:` / `いま:` / `次:` / `詰まり:` のラベル付き) をパースする。
    pub fn from_llm(text: &str, head: Head) -> Result<Self> {
        let mut parts: [String; 4] = Default::default();
        let mut cur: Option<usize> = None;
        for raw in text.lines() {
            let line = raw.trim();
            let mut matched = false;
            for (i, label) in LABELS.iter().enumerate() {
                let stripped = line
                    .strip_prefix(label)
                    .or_else(|| line.strip_prefix(&label.replace(':', "：")));
                if let Some(rest) = stripped {
                    cur = Some(i);
                    parts[i].push_str(rest.trim());
                    parts[i].push('\n');
                    matched = true;
                    break;
                }
            }
            if !matched {
                if let Some(i) = cur {
                    parts[i].push_str(line);
                    parts[i].push('\n');
                }
            }
        }
        let [line, now, next, stuck] = parts.map(|p| p.trim().to_owned());
        if line.is_empty() {
            bail!("summarizer output has no `一行:` label");
        }
        let line = truncate_chars(&line.replace('\n', " "), LINE_MAX_CHARS);
        Ok(Self {
            head,
            line,
            now,
            next,
            stuck,
        })
    }
}

fn split_sections(body: &str) -> (String, std::collections::HashMap<String, String>) {
    let mut line = String::new();
    let mut sections = std::collections::HashMap::new();
    let mut cur: Option<String> = None;
    let mut buf = String::new();
    for l in body.lines() {
        if HEADINGS.contains(&l.trim()) {
            if let Some(h) = cur.take() {
                sections.insert(h, buf.trim().to_owned());
            } else {
                line = buf.trim().to_owned();
            }
            buf.clear();
            cur = Some(l.trim().to_owned());
        } else {
            buf.push_str(l);
            buf.push('\n');
        }
    }
    match cur {
        Some(h) => {
            sections.insert(h, buf.trim().to_owned());
        }
        None => line = buf.trim().to_owned(),
    }
    (line, sections)
}

pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head() -> Head {
        Head {
            updated_at: Timestamp::UNIX_EPOCH,
            journal_bytes: 10,
            log_bytes: 20,
        }
    }

    #[test]
    fn llm_parse_and_roundtrip() {
        let text = "一行: リトライ実装中。次はテスト\nいま: retry を書いている\n続き\n次: 単体テスト\n詰まり: なし\n";
        let d = Digest::from_llm(text, head()).unwrap();
        assert_eq!(d.line, "リトライ実装中。次はテスト");
        assert_eq!(d.now, "retry を書いている\n続き");
        assert_eq!(d.next, "単体テスト");
        assert_eq!(d.stuck, "なし");
        let back = Digest::parse(&d.render().unwrap()).unwrap();
        assert_eq!(back.line, d.line);
        assert_eq!(back.now, d.now);
        assert_eq!(back.next, d.next);
        assert_eq!(back.stuck, d.stuck);
        assert_eq!(back.head.journal_bytes, 10);
    }

    #[test]
    fn stale() {
        let d = Digest::from_llm("一行: x", head()).unwrap();
        assert!(!d.is_stale(10, 20));
        assert!(d.is_stale(11, 20));
        assert!(d.is_stale(10, 21));
    }

    #[test]
    fn line_truncated() {
        let long = "あ".repeat(100);
        let d = Digest::from_llm(&format!("一行: {long}"), head()).unwrap();
        assert_eq!(d.line.chars().count(), LINE_MAX_CHARS);
    }

    #[test]
    fn missing_line_is_error() {
        assert!(Digest::from_llm("いま: x", head()).is_err());
    }
}
