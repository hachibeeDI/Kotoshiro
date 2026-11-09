use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use jiff::Timestamp;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Note,
    Check,
    Plan,
    Start,
    End,
    Status,
    Done,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Note => "note",
            Kind::Check => "check",
            Kind::Plan => "plan",
            Kind::Start => "start",
            Kind::End => "end",
            Kind::Status => "status",
            Kind::Done => "done",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub ts: Timestamp,
    pub session: Option<String>,
    pub kind: String,
    pub text: String,
}

/// `## <RFC3339> session=<8 chars> kind=<kind>` の見出し + 本文を追記する。
pub fn append(
    path: &Path,
    now: Timestamp,
    kind: Kind,
    session: Option<&str>,
    text: &str,
) -> Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut head = format!("## {now}");
    if let Some(s) = session {
        head.push_str(&format!(" session={}", short(s)));
    }
    head.push_str(&format!(" kind={}", kind.as_str()));
    write!(f, "{head}\n\n{}\n\n", text.trim_end())?;
    Ok(())
}

pub fn short(session_id: &str) -> &str {
    let end = session_id
        .char_indices()
        .nth(8)
        .map(|(i, _)| i)
        .unwrap_or(session_id.len());
    &session_id[..end]
}

pub fn read(path: &Path) -> Result<Vec<Entry>> {
    let s = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    Ok(parse(&s))
}

pub fn parse(s: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut buf = String::new();
    let flush = |out: &mut Vec<Entry>, buf: &mut String| {
        if let Some(last) = out.last_mut() {
            last.text = buf.trim().to_owned();
        }
        buf.clear();
    };
    for line in s.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            if let Some(e) = parse_head(head) {
                flush(&mut out, &mut buf);
                out.push(e);
                continue;
            }
        }
        if !out.is_empty() {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    flush(&mut out, &mut buf);
    out
}

fn parse_head(head: &str) -> Option<Entry> {
    let mut parts = head.split_whitespace();
    let ts: Timestamp = parts.next()?.parse().ok()?;
    let mut session = None;
    let mut kind = String::new();
    for p in parts {
        if let Some(v) = p.strip_prefix("session=") {
            session = Some(v.to_owned());
        } else if let Some(v) = p.strip_prefix("kind=") {
            kind = v.to_owned();
        }
    }
    Some(Entry {
        ts,
        session,
        kind,
        text: String::new(),
    })
}

/// 末尾 n 件。
pub fn tail(path: &Path, n: usize) -> Result<Vec<Entry>> {
    let mut all = read(path)?;
    let start = all.len().saturating_sub(n);
    Ok(all.split_off(start))
}

/// 指定バイト位置以降の生テキスト (digest の入力用)。
pub fn since(path: &Path, offset: u64) -> Result<String> {
    let s = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    let start = (offset as usize).min(s.len());
    Ok(String::from_utf8_lossy(&s[start..]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("journal.md");
        let t0 = Timestamp::UNIX_EPOCH;
        append(&p, t0, Kind::Note, Some("abcdefgh-1234"), "first\nline2").unwrap();
        append(
            &p,
            t0,
            Kind::Check,
            None,
            "## not a heading inside\n- [x] two",
        )
        .unwrap();
        let es = read(&p).unwrap();
        assert_eq!(es.len(), 2);
        assert_eq!(es[0].kind, "note");
        assert_eq!(es[0].session.as_deref(), Some("abcdefgh"));
        assert_eq!(es[0].text, "first\nline2");
        assert_eq!(es[1].kind, "check");
        assert!(es[1].text.contains("not a heading"));
        assert_eq!(tail(&p, 1).unwrap()[0].kind, "check");
    }
}
