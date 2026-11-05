use anyhow::{bail, Result};
use jiff::tz::TimeZone;
use jiff::Timestamp;

/// `org/repo#N` の形式検査。
pub fn parse_issue(s: &str) -> Result<String> {
    let Some((repo, num)) = s.split_once('#') else {
        bail!("issue must be org/repo#N, got `{s}`");
    };
    let ok_repo = repo
        .split_once('/')
        .is_some_and(|(o, r)| !o.is_empty() && !r.is_empty());
    if !ok_repo || num.parse::<u64>().is_err() {
        bail!("issue must be org/repo#N, got `{s}`");
    }
    Ok(s.to_owned())
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c))
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn local(ts: Timestamp) -> String {
    ts.to_zoned(TimeZone::system())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

/// "3m" / "2h" / "5d" 形式の経過時間。
pub fn age(now: Timestamp, then: Timestamp) -> String {
    let secs = (now.as_second() - then.as_second()).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// 端末幅の概算。CJK を 2、他を 1 と数える。
pub fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

fn char_width(c: char) -> usize {
    let u = c as u32;
    if (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE4F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x1F300..=0x1FAFF).contains(&u)
        || (0x20000..=0x3FFFD).contains(&u)
    {
        2
    } else {
        1
    }
}

/// 表示幅で切り詰め、右を空白で埋める。
pub fn fit(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push_str(&" ".repeat(width.saturating_sub(w)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_format() {
        assert!(parse_issue("org/repo#12").is_ok());
        assert!(parse_issue("org/repo").is_err());
        assert!(parse_issue("repo#12").is_err());
        assert!(parse_issue("org/repo#x").is_err());
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("abc-1"), "abc-1");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn fit_cjk() {
        assert_eq!(fit("日本語abc", 5), "日本 ");
        assert_eq!(fit("ab", 4), "ab  ");
    }
}
