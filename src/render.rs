use jiff::Timestamp;

use crate::store::digest::Digest;
use crate::store::journal::Entry;
use crate::store::sessions::Session;
use crate::store::todo::{Item, Status, Todo};
use crate::util::{age, display_width, fit, local};

pub struct Row {
    pub id: u64,
    pub status: Status,
    pub blocked: bool,
    pub title: String,
    pub line: Option<String>,
    pub stale: bool,
    pub updated_at: Timestamp,
    pub live: usize,
}

pub fn list(rows: &[Row], now: Timestamp) -> String {
    if rows.is_empty() {
        return "(no todos)\n".into();
    }
    let idw = rows
        .iter()
        .map(|r| r.id.to_string().len())
        .max()
        .unwrap_or(1)
        .max(2);
    let titlew = rows
        .iter()
        .map(|r| display_width(&r.title))
        .max()
        .unwrap_or(5)
        .clamp(5, 32);
    let mut out = String::new();
    out.push_str(&format!(
        "{:>idw$}  {:<8} {} {} {:>4} {:>2}  {}\n",
        "ID",
        "STATUS",
        "B",
        fit("TITLE", titlew),
        "AGE",
        "S",
        "DIGEST"
    ));
    for r in rows {
        let line = match &r.line {
            Some(l) if r.stale => format!("~{l}"),
            Some(l) => l.clone(),
            None => "-".into(),
        };
        out.push_str(&format!(
            "{:>idw$}  {:<8} {} {} {:>4} {:>2}  {}\n",
            r.id,
            r.status.to_string(),
            if r.blocked { "B" } else { " " },
            fit(&r.title, titlew),
            age(now, r.updated_at),
            r.live,
            line
        ));
    }
    out
}

pub struct ShowView<'a> {
    pub todo: &'a Todo,
    pub blocked: bool,
    pub digest: Option<&'a Digest>,
    pub stale: bool,
    pub checklist: &'a [Item],
    pub sessions: &'a [Session],
    pub journal: &'a [Entry],
    pub now: Timestamp,
}

pub fn show(v: &ShowView) -> String {
    let m = &v.todo.meta;
    let mut o = String::new();
    o.push_str(&format!("#{} {}\n", m.id, m.title));
    o.push_str(&format!(
        "status: {}{}   updated: {} ({} ago)\n",
        m.status,
        if v.blocked { " (blocked)" } else { "" },
        local(m.updated_at),
        age(v.now, m.updated_at)
    ));
    if let Some(i) = &m.issue {
        o.push_str(&format!("issue: {i}\n"));
    }
    if !m.repos.is_empty() {
        let rs: Vec<String> = m
            .repos
            .iter()
            .map(|r| format!("{} ({}, base {})", r.name, r.path.display(), r.base_branch))
            .collect();
        o.push_str(&format!("repos: {}\n", rs.join(", ")));
    }
    if let Some(p) = m.parent {
        o.push_str(&format!("parent: #{p}\n"));
    }
    if !m.blocks.is_empty() {
        o.push_str(&format!("blocks: {}\n", ids(&m.blocks)));
    }
    if !m.blocked_by.is_empty() {
        o.push_str(&format!("blocked_by: {}\n", ids(&m.blocked_by)));
    }

    o.push('\n');
    match v.digest {
        Some(d) => {
            o.push_str(&format!(
                "## digest{} ({})\n{}\n\nいま: {}\n次: {}\n詰まり: {}\n",
                if v.stale { " [stale]" } else { "" },
                local(d.head.updated_at),
                d.line,
                indent(&d.now),
                indent(&d.next),
                indent(&d.stuck)
            ));
        }
        None => o.push_str("## digest\n(none yet: run `kotoshiro digest`)\n"),
    }

    o.push_str("\n## checklist\n");
    if v.checklist.is_empty() {
        o.push_str("(empty)\n");
    }
    for (i, it) in v.checklist.iter().enumerate() {
        o.push_str(&format!(
            "{:>2}. [{}] {}\n",
            i + 1,
            if it.done { 'x' } else { ' ' },
            it.text
        ));
    }

    if !v.sessions.is_empty() {
        o.push_str("\n## sessions\n");
        for s in v.sessions {
            o.push_str(&format!(
                "{} {} {}{}{}\n",
                if s.is_live() { "*" } else { " " },
                &s.session_id[..s.session_id.len().min(8)],
                local(s.started_at),
                s.repo
                    .as_deref()
                    .map(|r| format!(" repo={r}"))
                    .unwrap_or_default(),
                s.ended_at
                    .map(|e| format!(" ended {}", local(e)))
                    .unwrap_or_default()
            ));
        }
    }

    if !v.journal.is_empty() {
        o.push_str("\n## journal (tail)\n");
        for e in v.journal {
            o.push_str(&format!(
                "- {} [{}{}] {}\n",
                local(e.ts),
                e.kind,
                e.session
                    .as_deref()
                    .map(|s| format!(" {s}"))
                    .unwrap_or_default(),
                indent(&e.text)
            ));
        }
    }
    o
}

fn ids(v: &[u64]) -> String {
    v.iter()
        .map(|i| format!("#{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn indent(s: &str) -> String {
    s.trim().replace('\n', "\n      ")
}
