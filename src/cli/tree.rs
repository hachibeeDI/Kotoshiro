//! parent で木を組んで表示する。

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use clap::Args as ClapArgs;
use serde::Serialize;

use super::Ctx;
use crate::store::todo::{Meta, Status};

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// この TODO を根にする (省略時は親を持たない TODO 全部)
    pub id: Option<u64>,
    /// done も含める
    #[arg(long)]
    pub all: bool,
}

#[derive(Serialize)]
pub struct Node {
    pub id: u64,
    pub title: String,
    pub status: Status,
    pub blocked: bool,
    pub digest: Option<String>,
    pub stale: bool,
    pub children: Vec<Node>,
}

struct Info {
    meta: Meta,
    blocked: bool,
    digest: Option<String>,
    stale: bool,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    let map = ctx.status_map()?;
    let mut infos: BTreeMap<u64, Info> = BTreeMap::new();
    for id in ctx.store.ids()? {
        let t = match ctx.load(id) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("warning: skip #{id}: {e:#}");
                continue;
            }
        };
        let (d, stale) = ctx.digest_state(id)?;
        infos.insert(
            id,
            Info {
                blocked: ctx.blocked(&t.meta, &map),
                digest: d.map(|d| d.line),
                stale,
                meta: t.meta,
            },
        );
    }
    let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
    for (id, i) in &infos {
        if let Some(p) = i.meta.parent.filter(|p| infos.contains_key(p)) {
            children.entry(p).or_default().push(*id);
        }
    }
    let roots: Vec<u64> = match a.id {
        Some(id) => {
            ctx.load(id)?;
            vec![id]
        }
        None => infos
            .iter()
            .filter(|(_, i)| !i.meta.parent.is_some_and(|p| infos.contains_key(&p)))
            .map(|(id, _)| *id)
            .collect(),
    };
    let nodes: Vec<Node> = roots
        .iter()
        .filter_map(|id| build(*id, &infos, &children, a.all || a.id == Some(*id)))
        .collect();

    if ctx.json {
        ctx.print_json(&nodes)
    } else {
        if nodes.is_empty() {
            println!("(no todos)");
        }
        print!("{}", render(&nodes));
        Ok(())
    }
}

fn build(
    id: u64,
    infos: &BTreeMap<u64, Info>,
    children: &HashMap<u64, Vec<u64>>,
    all: bool,
) -> Option<Node> {
    let i = infos.get(&id)?;
    let kids: Vec<Node> = children
        .get(&id)
        .map(|v| {
            v.iter()
                .filter_map(|c| build(*c, infos, children, all))
                .collect()
        })
        .unwrap_or_default();
    if !all && i.meta.status == Status::Done && kids.is_empty() {
        return None;
    }
    Some(Node {
        id,
        title: i.meta.title.clone(),
        status: i.meta.status,
        blocked: i.blocked,
        digest: i.digest.clone(),
        stale: i.stale,
        children: kids,
    })
}

pub fn render(nodes: &[Node]) -> String {
    let mut o = String::new();
    for n in nodes {
        line(&mut o, n, "", "");
        kids(&mut o, &n.children, "");
    }
    o
}

fn kids(o: &mut String, nodes: &[Node], prefix: &str) {
    let n = nodes.len();
    for (i, c) in nodes.iter().enumerate() {
        let last = i + 1 == n;
        line(o, c, prefix, if last { "└─ " } else { "├─ " });
        let next = format!("{prefix}{}", if last { "   " } else { "│  " });
        kids(o, &c.children, &next);
    }
}

fn line(o: &mut String, n: &Node, prefix: &str, branch: &str) {
    o.push_str(&format!(
        "{prefix}{branch}#{} {:<8}{} {}{}\n",
        n.id,
        n.status.to_string(),
        if n.blocked { " B" } else { "  " },
        n.title,
        match &n.digest {
            Some(d) => format!("  {}{d}", if n.stale { "~" } else { "" }),
            None => String::new(),
        }
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, children: Vec<Node>) -> Node {
        Node {
            id,
            title: format!("t{id}"),
            status: Status::Active,
            blocked: id == 3,
            digest: (id == 2).then(|| "d".into()),
            stale: true,
            children,
        }
    }

    #[test]
    fn render_shape() {
        let tree = vec![node(
            1,
            vec![node(2, vec![node(4, vec![])]), node(3, vec![])],
        )];
        let s = render(&tree);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "#1 active     t1");
        assert_eq!(lines[1], "├─ #2 active     t2  ~d");
        assert_eq!(lines[2], "│  └─ #4 active     t4");
        assert_eq!(lines[3], "└─ #3 active   B t3");
    }
}
