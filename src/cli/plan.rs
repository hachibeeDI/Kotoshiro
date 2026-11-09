//! 箇条書きを checklist (または子 TODO) に落とす。セッションが計画確定時に打つ。

use std::io::Read;

use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use serde::Serialize;

use super::Ctx;
use crate::resolve;
use crate::store::journal::{self, Kind};
use crate::store::todo::{Item, Status, Todo};
use crate::store::{counter, lock};

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// 箇条書き (- / * / 1.)。省略または `-` で stdin
    #[arg(allow_hyphen_values = true)]
    pub text: Option<String>,
    /// 対象 TODO (既定: KOTOSHIRO_TODO)
    #[arg(long, value_name = "ID")]
    pub todo: Option<u64>,
    /// checklist ではなく子 TODO として起票する (repos は親から継承)
    #[arg(long)]
    pub children: bool,
    /// 未消化の checklist を消してから足す
    #[arg(long, conflicts_with = "children")]
    pub replace: bool,
}

#[derive(Serialize)]
struct Out {
    id: u64,
    items: Vec<String>,
    children: Vec<u64>,
    checklist: Vec<Item>,
    status: Status,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let text = match a.text.as_deref() {
        None | Some("-") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        }
        Some(t) => t.to_owned(),
    };
    let items = parse_items(&text);
    if items.is_empty() {
        bail!("no list items found (use `- `, `* ` or `1. ` bullets at column 0)");
    }

    let cur = resolve::require(&ctx.store, a.todo)?;
    let _g = lock::lock_todo(&ctx.store, cur.id)?;
    let mut t = ctx.load(cur.id)?;

    let mut children = Vec::new();
    let mut note = String::new();
    if a.children {
        for it in &items {
            let id = counter::next_id(&ctx.store)?;
            let mut c = Todo::new(id, it.as_str(), ctx.now);
            c.meta.parent = Some(cur.id);
            c.meta.repos = t.meta.repos.clone();
            ctx.save(&c)?;
            children.push(id);
            note.push_str(&format!("- #{id} {it}\n"));
        }
        note.insert_str(0, &format!("plan: {} child TODO(s)\n", items.len()));
    } else {
        if a.replace {
            t.remove_unchecked();
        }
        for it in &items {
            t.push_item(it);
        }
        note.push_str(&format!(
            "plan: {} item(s){}\n",
            items.len(),
            if a.replace { " (replaced)" } else { "" }
        ));
        for it in &items {
            note.push_str(&format!("- {it}\n"));
        }
    }
    if t.meta.status == Status::Inbox {
        t.meta.status = Status::Planning;
    }
    t.touch(ctx.now);
    ctx.save(&t)?;
    journal::append(
        &ctx.store.journal(cur.id),
        ctx.now,
        Kind::Plan,
        cur.session_id.as_deref(),
        &note,
    )?;

    if ctx.json {
        ctx.print_json(&Out {
            id: cur.id,
            items,
            children,
            checklist: t.checklist(),
            status: t.meta.status,
        })
    } else {
        if a.children {
            println!("#{}: {} child TODO(s) created", cur.id, children.len());
            for (id, it) in children.iter().zip(&items) {
                println!("  #{id} {it}");
            }
        } else {
            println!("#{}: {} item(s) added", cur.id, items.len());
            for (i, it) in t.checklist().iter().enumerate() {
                println!(
                    "{:>2}. [{}] {}",
                    i + 1,
                    if it.done { 'x' } else { ' ' },
                    it.text
                );
            }
        }
        Ok(())
    }
}

/// 行頭 (インデントなし) の箇条書きだけを拾う。入れ子は無視する。`- [ ] x` 形式も受ける。
pub fn parse_items(text: &str) -> Vec<String> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<String> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .or_else(|| strip_number(line))?;
    let rest = rest
        .strip_prefix("[ ] ")
        .or_else(|| rest.strip_prefix("[x] "))
        .or_else(|| rest.strip_prefix("[X] "))
        .unwrap_or(rest);
    let t = rest.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// `1. x` / `12) x`
fn strip_number(line: &str) -> Option<&str> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &line[digits..];
    rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullets() {
        let text = "計画:\n- one\n  - nested\n* two\n1. three\n12) four\n- [ ] five\n- [x] six\n\n-not\n7 no\n";
        assert_eq!(
            parse_items(text),
            vec!["one", "two", "three", "four", "five", "six"]
        );
        assert!(parse_items("just prose").is_empty());
    }
}
