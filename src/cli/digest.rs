use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use super::Ctx;
use crate::store::digest::{Digest, Head};
use crate::store::journal;
use crate::store::lock;
use crate::store::paths::{dir_bytes, file_bytes};
use crate::store::sessions;
use crate::summarizer;
use crate::util::local;

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub id: u64,
    /// 失敗しても exit 0 (hook から呼ぶ時用)
    #[arg(long)]
    pub quiet: bool,
}

const LOG_TAIL_BYTES: u64 = 4096;
const LOG_SESSIONS: usize = 3;

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    match generate(ctx, a.id) {
        Ok(d) => {
            if ctx.json {
                ctx.print_json(&d)
            } else {
                println!("#{} digest updated: {}", a.id, d.line);
                Ok(())
            }
        }
        Err(e) => {
            eprintln!("warning: digest for #{} not updated: {e:#}", a.id);
            if a.quiet {
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

pub fn generate(ctx: &Ctx, id: u64) -> Result<Digest> {
    let _g = lock::lock_todo(&ctx.store, id)?;
    let todo = ctx.load(id)?;
    let prev = Digest::load(&ctx.store.digest(id))?;
    let jpath = ctx.store.journal(id);
    let journal_bytes = file_bytes(&jpath);
    let log_bytes = dir_bytes(&ctx.store.log_dir(id));

    let mut input = String::new();
    input.push_str(&format!(
        "# TODO #{}: {}\n\nstatus: {}\n",
        id, todo.meta.title, todo.meta.status
    ));
    if let Some(i) = &todo.meta.issue {
        input.push_str(&format!("issue: {i}\n"));
    }
    input.push_str("\n## checklist\n");
    for it in todo.checklist() {
        input.push_str(&format!(
            "- [{}] {}\n",
            if it.done { 'x' } else { ' ' },
            it.text
        ));
    }
    if let Some(p) = &prev {
        input.push_str(&format!(
            "\n## 前回の digest ({})\n一行: {}\nいま: {}\n次: {}\n詰まり: {}\n",
            local(p.head.updated_at),
            p.line,
            p.now,
            p.next,
            p.stuck
        ));
    }
    let since = prev.as_ref().map(|p| p.head.journal_bytes).unwrap_or(0);
    let new_journal = journal::since(&jpath, since)?;
    input.push_str("\n## journal (前回 digest 以降)\n");
    if new_journal.trim().is_empty() {
        input.push_str("(追記なし)\n");
    } else {
        input.push_str(&new_journal);
    }
    let tail = journal::tail(&jpath, 3)?;
    if since > 0 && !tail.is_empty() {
        input.push_str("\n## journal (直近 3 件、文脈用)\n");
        for e in tail {
            input.push_str(&format!("- {} [{}] {}\n", local(e.ts), e.kind, e.text));
        }
    }

    let mut list = sessions::load(&ctx.store.sessions(id))?;
    list.sort_by_key(|s| std::cmp::Reverse(s.started_at));
    let mut logs = String::new();
    for s in list.iter().take(LOG_SESSIONS) {
        let p = ctx.store.log_file(id, &s.session_id);
        let t = tail_bytes(&p, LOG_TAIL_BYTES)?;
        if !t.trim().is_empty() {
            logs.push_str(&format!(
                "### session {} ({})\n{}\n",
                journal::short(&s.session_id),
                local(s.started_at),
                t
            ));
        }
    }
    if !logs.is_empty() {
        input.push_str("\n## 生ログ (アシスタントの発話末尾)\n");
        input.push_str(&logs);
    }

    let s = summarizer::select(&ctx.config);
    let out = s
        .summarize(PROMPT, &input)
        .with_context(|| format!("summarizer `{}`", s.name()))?;
    let d = Digest::from_llm(
        &out,
        Head {
            updated_at: ctx.now,
            journal_bytes,
            log_bytes,
        },
    )?;
    d.save(&ctx.store.digest(id))?;
    Ok(d)
}

fn tail_bytes(path: &Path, n: u64) -> Result<String> {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(e).with_context(|| format!("open {}", path.display())),
    };
    let len = f.metadata()?.len();
    let start = len.saturating_sub(n);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let mut s = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        // 途中から始まるので最初の行は捨てる
        if let Some(i) = s.find('\n') {
            s = s[i + 1..].to_owned();
        }
    }
    Ok(s)
}

const PROMPT: &str = "\
あなたは作業ログの要約器。stdin に 1 つの TODO の title、checklist、前回の digest、journal (作業セッション自身が書いた進捗)、生ログ (アシスタント発話の末尾) が渡される。
日本語で、次の 4 つのラベルだけを使って出力する。前置き、説明、markdown 装飾、ラベル以外の行頭は禁止。

一行: いま何をしていて次は何かを 80 字以内の 1 行で
いま: 現在の作業状態を 1〜3 行
次: 次にやることを 1〜3 行
詰まり: 障害や未解決の疑問を 1〜3 行。無ければ「なし」

根拠の優先順位は journal > 生ログ > 前回 digest。checklist の未完了項目を「次」の根拠にする。推測で補わない。";
