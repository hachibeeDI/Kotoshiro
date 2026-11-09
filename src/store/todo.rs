use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Inbox,
    Planning,
    Active,
    Done,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Status::Inbox => "inbox",
            Status::Planning => "planning",
            Status::Active => "active",
            Status::Done => "done",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub name: String,
    pub path: PathBuf,
    pub base_branch: String,
}

impl Repo {
    /// `<repo>/../<repo名>-wt/todo-<id>`
    pub fn worktree_dir(&self, id: u64) -> PathBuf {
        let parent = self.path.parent().unwrap_or(&self.path);
        parent
            .join(format!("{}-wt", self.name))
            .join(format!("todo-{id}"))
    }

    pub fn branch(&self, id: u64) -> String {
        format!("todo/{id}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pr {
    pub repo: String,
    pub number: u64,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub id: u64,
    pub title: String,
    pub status: Status,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    #[serde(default)]
    pub repos: Vec<Repo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    #[serde(default)]
    pub blocks: Vec<u64>,
    #[serde(default)]
    pub blocked_by: Vec<u64>,
    #[serde(default)]
    pub prs: Vec<Pr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub done: bool,
    pub text: String,
}

/// todo.md = YAML frontmatter + 本文。checklist は本文中の `- [ ]` / `- [x]` 行を
/// その場で読み書きする (本文の他の部分は触らない)。
#[derive(Debug, Clone)]
pub struct Todo {
    pub meta: Meta,
    pub body: String,
}

const CHECKLIST_HEADING: &str = "## Checklist";

impl Todo {
    pub fn new(id: u64, title: impl Into<String>, now: Timestamp) -> Self {
        Self {
            meta: Meta {
                id,
                title: title.into(),
                status: Status::Inbox,
                created_at: now,
                updated_at: now,
                issue: None,
                repos: Vec::new(),
                parent: None,
                blocks: Vec::new(),
                blocked_by: Vec::new(),
                prs: Vec::new(),
            },
            body: format!("\n{CHECKLIST_HEADING}\n\n"),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let s =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::parse(&s).with_context(|| format!("parse {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
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
        let meta: Meta = serde_yaml::from_str(front).context("frontmatter")?;
        Ok(Self {
            meta,
            body: body.to_owned(),
        })
    }

    pub fn render(&self) -> Result<String> {
        let front = serde_yaml::to_string(&self.meta)?;
        Ok(format!("---\n{front}---\n{}", self.body))
    }

    pub fn checklist(&self) -> Vec<Item> {
        self.body.lines().filter_map(parse_item).collect()
    }

    /// n は 1 始まり。
    pub fn set_checked(&mut self, n: usize, done: bool) -> Result<Item> {
        if n == 0 {
            bail!("checklist index starts at 1");
        }
        let mut lines: Vec<String> = self.body.lines().map(str::to_owned).collect();
        let mut seen = 0usize;
        for line in lines.iter_mut() {
            if let Some(item) = parse_item(line) {
                seen += 1;
                if seen == n {
                    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                    let mark = if done { 'x' } else { ' ' };
                    *line = format!("{indent}- [{mark}] {}", item.text);
                    let trailing = if self.body.ends_with('\n') { "\n" } else { "" };
                    self.body = lines.join("\n") + trailing;
                    return Ok(Item {
                        done,
                        text: item.text,
                    });
                }
            }
        }
        bail!("checklist has {seen} items, no item {n}")
    }

    /// 未消化の checklist 行を消す (`plan --replace`)。消化済みは残す。
    pub fn remove_unchecked(&mut self) -> usize {
        let before = self.body.lines().count();
        let trailing = if self.body.ends_with('\n') { "\n" } else { "" };
        let kept: Vec<&str> = self
            .body
            .lines()
            .filter(|l| !matches!(parse_item(l), Some(Item { done: false, .. })))
            .collect();
        let removed = before - kept.len();
        self.body = kept.join("\n") + trailing;
        removed
    }

    /// 本文末尾 (`## Checklist` 配下) に項目を足す。
    pub fn push_item(&mut self, text: &str) {
        if !self.body.contains(CHECKLIST_HEADING) {
            if !self.body.ends_with('\n') {
                self.body.push('\n');
            }
            self.body.push_str(&format!("\n{CHECKLIST_HEADING}\n\n"));
        }
        if !self.body.ends_with('\n') {
            self.body.push('\n');
        }
        self.body.push_str(&format!("- [ ] {text}\n"));
    }

    pub fn touch(&mut self, now: Timestamp) {
        self.meta.updated_at = now;
    }
}

fn parse_item(line: &str) -> Option<Item> {
    let t = line.trim_start();
    let rest = t.strip_prefix("- [")?;
    let (mark, text) = rest.split_at_checked(1)?;
    let text = text.strip_prefix("] ")?;
    let done = match mark {
        " " => false,
        "x" | "X" => true,
        _ => return None,
    };
    Some(Item {
        done,
        text: text.trim_end().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Todo {
        let mut t = Todo::new(42, "試験", Timestamp::UNIX_EPOCH);
        t.push_item("one");
        t.push_item("two");
        t
    }

    #[test]
    fn roundtrip() {
        let t = sample();
        let s = t.render().unwrap();
        let back = Todo::parse(&s).unwrap();
        assert_eq!(back.meta.id, 42);
        assert_eq!(back.meta.title, "試験");
        assert_eq!(back.body, t.body);
        assert_eq!(back.checklist().len(), 2);
    }

    #[test]
    fn check_toggle() {
        let mut t = sample();
        t.set_checked(2, true).unwrap();
        let items = t.checklist();
        assert!(!items[0].done);
        assert!(items[1].done);
        t.set_checked(1, true).unwrap();
        assert!(t.checklist().iter().all(|i| i.done));
        assert!(t.set_checked(3, true).is_err());
        assert!(t.set_checked(0, true).is_err());
    }

    #[test]
    fn remove_unchecked_keeps_done() {
        let mut t = sample();
        t.set_checked(1, true).unwrap();
        assert_eq!(t.remove_unchecked(), 1);
        let items = t.checklist();
        assert_eq!(items.len(), 1);
        assert!(items[0].done);
        t.push_item("three");
        assert_eq!(t.checklist().len(), 2);
    }

    #[test]
    fn worktree_dir_rule() {
        let r = Repo {
            name: "app".into(),
            path: PathBuf::from("/src/app"),
            base_branch: "main".into(),
        };
        assert_eq!(r.worktree_dir(42), PathBuf::from("/src/app-wt/todo-42"));
        assert_eq!(r.branch(42), "todo/42");
    }

    #[test]
    fn body_outside_checklist_preserved() {
        let mut t = sample();
        t.body = format!("説明文\n\n- not an item\n{}", t.body);
        t.set_checked(1, true).unwrap();
        assert!(t.body.starts_with("説明文\n\n- not an item\n"));
    }
}
