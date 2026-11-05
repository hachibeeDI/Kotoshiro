mod agent;
mod cli;
mod git;
mod hook;
mod proc;
mod render;
mod resolve;
mod runtime;
mod store;
mod summarizer;
mod util;

use clap::{Parser, Subcommand};

use cli::Ctx;
use store::Store;

/// TODO を根に据えて Claude Code セッション群を司るツール
#[derive(Parser, Debug)]
#[command(name = "kotoshiro", version, about)]
struct Cli {
    /// 機械読み用 JSON で出力
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// ストア作成、Claude Code の hook / skill を導入、`todo` リンクを張る
    Init(cli::init::Args),
    /// TODO を作る
    New(cli::new::Args),
    /// GitHub issue を後付けする
    Link(cli::link::Args),
    /// worktree を用意してセッション起動コマンドを出す
    Start(cli::start::Args),
    /// journal に進捗を書く
    Note(cli::note::Args),
    /// checklist をトグルする
    Check(cli::check::Args),
    /// status を変える
    Status(cli::status::Args),
    /// Claude Code hook のエントリ (stdin に JSON)
    #[command(subcommand)]
    Hook(cli::hook::Event),
    /// digest を (再) 生成する
    Digest(cli::digest::Args),
    /// TODO 一覧
    List(cli::list::Args),
    /// TODO の詳細
    Show(cli::show::Args),
}

fn main() {
    let cli = Cli::parse();
    let is_hook = matches!(cli.cmd, Cmd::Hook(_));
    let r = Store::from_env().and_then(|store| {
        let ctx = Ctx {
            store,
            json: cli.json,
            now: jiff::Timestamp::now(),
        };
        match cli.cmd {
            Cmd::Init(a) => cli::init::run(&ctx, a),
            Cmd::New(a) => cli::new::run(&ctx, a),
            Cmd::Link(a) => cli::link::run(&ctx, a),
            Cmd::Start(a) => cli::start::run(&ctx, a),
            Cmd::Note(a) => cli::note::run(&ctx, a),
            Cmd::Check(a) => cli::check::run(&ctx, a),
            Cmd::Status(a) => cli::status::run(&ctx, a),
            Cmd::Hook(ev) => cli::hook::run(&ctx, ev),
            Cmd::Digest(a) => cli::digest::run(&ctx, a),
            Cmd::List(a) => cli::list::run(&ctx, a),
            Cmd::Show(a) => cli::show::run(&ctx, a),
        }
    });
    if let Err(e) = r {
        eprintln!("error: {e:#}");
        if !is_hook {
            std::process::exit(1);
        }
    }
}
