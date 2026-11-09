# kotoshiro (事代主) — 設計メモ

TODO を根に据えて Claude Code セッション群を司るツール。
名は事代主神 (事を代わって知らす主) から。

## 動機 (2026-11-05 時点の痛み)

- 1 issue に複数セッションを立てると管理が手動
- セッションごとのサマリーがなく、並列作業中にログを読むのが苦痛
- 「TODO に対する進捗」を脳内で管理している。日が空くと曖昧になる

## 前提環境

- issue は GitHub 上で biz メンバーと共同管理 (source of truth)
- 作業は ghostty + herdr + Claude Code
- 既存ツール (kanban-code, Agent Teams, CCPM 等) はセッション中心で、TODO を根にしたものはなかった

## 役割分担

| 層 | 担当 | 備考 |
|---|---|---|
| TODO ストア / CLI / hook / skill | kotoshiro | ここだけ作る |
| セッション起動・pane 管理・状態検知 | Runtime port (初期実装: herdr) | 下記「Runtime 抽象化」。ターミナル (ghostty 等) には依存しない |
| セッション間通信 | Claude Code cross-session messaging (`ListAgents` / `SendMessage`) | 通信層は書かない |
| 進捗収集の起点 | Claude Code Stop / SessionEnd hook | Stop は `last_assistant_message` を生ログに追記するだけ。SessionEnd が digest 生成を起動 |

## Runtime 抽象化

周辺ツールへの依存は port として切り、kotoshiro 本体は port だけを見る。
herdr は最初の adapter であり前提ではない。

```rust
enum SessionState { Working, Blocked, Idle, Unknown }  // 終了は sessions.json の ended_at で持つ

// 不透明な参照。adapter 固有の id (herdr なら pane "w1:p1") を中に持つ
struct SessionRef { runtime: String, r#ref: String }

// adapter が workspace を作る時に使う TODO 側の情報
struct Workspace<'a> { id: u64, title: &'a str, repo: Option<&'a str>, focus: bool }

enum Spawn { Exec, Started(SessionRef) }  // Exec = 呼び出し元プロセスが claude に差し替わる (manual)

trait Runtime {
    fn name(&self) -> &'static str;
    fn spawn(&self, launch: &Launch, ws: &Workspace) -> Result<Spawn>;
    fn state(&self, s: &SessionRef) -> Result<SessionState>;
    fn close_workspace(&self, ws: &Workspace) -> Result<bool>;  // 無ければ false
}
```

prompt / wait / read / subscribe は driver セッション (Epic 自走) を作る段で足す。それまでは skill から直接 `herdr agent ...` を打てばよい。

- adapter: `manual` (既定。起動コマンドを組み立て、`start` はそのプロセスに差し替わる。`--print` なら表示する。CI やテストでも使う)、`herdr` (CLI を叩く。socket API は subscribe のみ)。`tmux` は herdr 不在時の候補、未実装
- adapter の選択は `start --runtime` > `KOTOSHIRO_RUNTIME` > `config.toml` の `runtime` > manual。自動検出はしない (暗黙の依存を作らない)
- `sessions.json` には `SessionRef` をそのまま保存する。adapter が変わっても過去レコードは読める
- どの adapter でも `start --print` は manual 形式のシェルコマンドを出す。どこでも打てる最終手段として

### herdr との対応

| kotoshiro | herdr | 理由 |
|---|---|---|
| TODO | workspace (label `#<id> <title>`、`--env KOTOSHIRO_TODO=<id>`) | cwd と env が workspace 単位。手で開いた pane も hook が拾う。片付けは `workspace close` 1 発 |
| repo (複数リポ TODO) | tab (label = repo 名、cwd = その repo の worktree) | 3b の決定通り |
| Claude セッション | pane 内の agent (`agent start <name> --kind claude --pane <id> -- <args>`) | 名前は `todo-<id>[-<repo>]-<session 先頭 4>`。herdr の名前規則 `[a-z][a-z0-9_-]{0,31}` に収め、live agent 間で一意にする |
| Epic (parent) | 対応なし | herdr に階層はない |

- workspace の herdr ID は herdr 再起動で変わりうるので保存しない。毎回 `workspace list` を label `#<id> ` 前置で引く。見つかれば tab を足し、無ければ workspace を作ってその root pane を使う
- herdr の worktree 機能 (`herdr worktree create`) は使わない。複数リポをまたぐので kotoshiro が `git worktree` を作り、herdr には `--cwd` で渡すだけ
- herdr の JSON は `--json` でフィールドを指定できないので未知フィールドは無視する (下記「実装方針」の例外)。必要なのは `result.workspace.workspace_id` / `result.root_pane.pane_id` / `result.tab.tab_id` / `result.agent.agent_status` と `workspaces[].label`
- `start` は既定で新 workspace にフォーカスを移す。driver セッションや裏で立てる時は `--no-focus`
- Claude Code 自体への依存も `Agent` port に切る余地を残す (`claude` コマンド名、`--resume` の形、hook の payload 形式)。初期は claude 固定で良いが、コマンド組み立ては 1 箇所に集める
- worktree 作成は herdr に任せず kotoshiro 側で `git worktree` を直接叩く。Runtime に依存させない

## 設計制約

- **issue に紐づかない単独 TODO は一級市民。** `issue` は任意フィールド。`todo new` は `--issue` なしで作れ、`todo link --issue` で後付けできる。`todo done` は issue がなければコメント投稿を単に省く
- TODO はリポジトリに属さない。session / worktree / PR はリポ単位、TODO はその上位。ストアは `~/todos` 等リポ外
- TODO 間の関係: `parent` (Epic) と `blocks` / `blocked_by`。`blocks` には閉路検査
- **blocked は導出値。** `blocked_by` に未完了 (status != done) の TODO があれば blocked。status には持たない (状態遷移の二重管理を避ける)
- セッションの権限: `note` と `check` は打ってよい。`status <id> done` は打たない。完了判断は人
- GitHub issue と TODO は N:N。issue 1 つから複数 TODO、1 TODO が複数リポの PR を持つ
- journal は内部専用 (biz の目を気にしない LLM 向け密度)
- **ユーザー向けの digest を別に持つ。** journal から機械生成する派生物で、手で書かない。週末をまたいで並列作業に戻る時に `todo list` / `todo show` で一目で状況が分かることが目的
- biz 向けの状態更新は完了時 1 回のみ。定期ステータス更新はしない

## digest (ユーザー向けサマリ)

- 粒度は 2 段:
  - **一行** (`todo list` に出す): 「いま何をしていて、次は何か」を 80 字以内。一言主の趣旨
  - **短文** (`todo show` に出す): 3 項目固定。`いま` / `次` / `詰まり`。各 1〜3 行
- 生成タイミング: SessionEnd hook (`kotoshiro hook session-end` が `kotoshiro digest <id>` を detach 起動) と手動 `kotoshiro digest <id>`。Stop hook では生成しない
- 入力: title + checklist + journal 末尾 (前回 digest 以降) + 生ログ差分。haiku に固定書式 (`一行:` / `いま:` / `次:` / `詰まり:`) で出させてパースする
- `digest.md` に `updated_at` と生成元の journal / log のバイト数を記録し、journal より古い digest を「stale」表示する
- 生成に失敗しても作業を止めない。前回の digest を残して警告だけ出す
- `todo list` の既定表示: status、blocked フラグ、一行 digest、updated_at からの経過時間、動作中セッション数

## 進捗収集 (journal / log / digest の 3 層)

`/recap` と同じ思想: 外部で transcript を解析するのではなく、文脈を持つセッション自身に書かせる。
transcript JSONL の行形式は公式に「内部仕様、バージョンで変わる」と明記されているので解析に依存しない。

| 層 | 書き手 | コスト | 用途 |
|---|---|---|---|
| journal.md | セッション自身 (skill 経由の `note` / `check`)、CLI (start / status の自動記録) | 0 | 進捗の正本。内部専用 |
| log/<session_id>.md | Stop hook (`last_assistant_message` をそのまま追記) | 0 (数 ms) | note の抜けを補う生データ。digest の入力 |
| digest.md | `kotoshiro digest` (haiku) | SessionEnd 毎 + 手動 | ユーザー向け |

## ストア

`$KOTOSHIRO_HOME` (既定 `~/todos`):

```
config.toml               # 設定 (下記)。init が雛形を書く
.counter                  # 連番 ID の次値。ロック下で読み書き
<id>/
  .lock                   # TODO ディレクトリ単位の排他 (std の File::lock)。2 TODO を触る時は ID 昇順に取る
  todo.md                 # YAML frontmatter + 本文 + "## Checklist"
  journal.md              # 追記専用。"## <RFC3339> session=<sid 先頭 8> kind=<note|check|plan|start|end|status|done>" + 本文。contract セクションにリポ間の契約
  digest.md               # frontmatter: updated_at, journal_bytes, log_bytes。本文: 一行 + いま/次/詰まり。手で編集しない
  sessions.json           # [{session_id, runtime: SessionRef, repo?, worktree?, cwd, started_at, ended_at?, log_bytes}]
  context.md              # start が生成。--append-system-prompt-file で渡す
  log/<session_id>.md     # Stop hook の生ログ
```

todo.md frontmatter: `id`, `title`, `status`, `created_at`, `updated_at`, `issue?`, `repos: [{name, path, base_branch}]`, `parent?`, `blocks[]`, `blocked_by[]`, `prs: [{repo, number, state}]`

`prs` は `done` が `gh pr list --head todo/<id>` で引いた結果を記録するもので、手では書かない。

status: inbox → planning → active → done (blocked は導出)

ID は連番。ストアはローカル前提。

### セッションと TODO の紐付け (優先順)

1. `kotoshiro start` が uuid を採番して `claude --session-id <uuid>` で渡し、sessions.json に先に記録する。hook の `session_id` と直接一致する
2. env `KOTOSHIRO_TODO` (`start` が起動コマンドに付ける。hook と Bash tool は claude プロセスの env を継承する)
3. hook の `cwd` が sessions.json の worktree に一致 (手で起動したセッションの拾い上げ)

どれにも該当しなければ hook は何もせず終了する。

### worktree

既定は `<repo>/../<repo名>-wt/todo-<id>` (config の `worktree` テンプレートで変更可)、ブランチ `todo/<id>`、base は `repos[].base_branch`。repos が空の TODO は worktree を作らず cwd で起動する。

### 設定 (`$KOTOSHIRO_HOME/config.toml`)

マシン単位の好みだけを持つ。TODO ごとのデータは todo.md、hook の設定は Claude Code 側。ストアの場所だけは bootstrap なので env `KOTOSHIRO_HOME` のまま。
優先順はコマンドのフラグ > 環境変数 > config > 既定。`init` がコメント付きの雛形を書く。未知のキーはエラー (typo を黙って無視しない)。hook だけは壊れた config でも既定で進む。

| キー | 既定 | 用途 |
|---|---|---|
| `runtime` | `manual` | セッション起動の adapter (env `KOTOSHIRO_RUNTIME`) |
| `worktree` | `{repo}/../{name}-wt/todo-{id}` | worktree の置き場。相対ならリポ基準 |
| `claude_config_dir` | 無し (claude の既定 `~/.claude`) | `init` の書き先。`start` が起動する claude と要約器の子プロセスにも `CLAUDE_CONFIG_DIR` として渡す。env の無い pane から打っても同じ設定・ログインを使うため (env `CLAUDE_CONFIG_DIR`) |

env で済むものをわざわざ config にした理由: `todo start` を env の無い pane で打つと manual に落ちてその pane で exec される、`init` を env の無い window で打つと `~/.claude` に書かれる、という事故が実際に起きた。設定ファイルなら pane や window に依らない。

## コマンド

バイナリ名は `kotoshiro`。`init` が同じディレクトリに `todo` シンボリックリンクを張る。skill 内の表記は `kotoshiro`。
全サブコマンドに `--json`。hook は常に exit 0。

```
kotoshiro init [--claude-config-dir PATH]
                            # ~/todos と config.toml 作成、Claude 設定 dir の settings.json に hook 追記、skills/kotoshiro/SKILL.md 配置、todo リンク
                            # Claude 設定 dir はフラグ > env CLAUDE_CONFIG_DIR > config.toml > ~/.claude。明示されたら config.toml に記録する
kotoshiro new <title> [--issue org/repo#N] [--repo <path>]... [--blocks <id>] [--parent <id>]
                            # --issue なしの単独 TODO が既定
kotoshiro link <id> --issue org/repo#N   # issue の後付け
kotoshiro start <id> [--repo <name>] [--print] [--dry-run] [--no-focus]
                            # worktree 作成 → context.md 生成 → uuid 採番 → sessions.json 記録 → Runtime で起動
                            # manual: claude に execvp。herdr: workspace/tab を作り agent start、pane id を sessions.json に書き戻す
                            # --print は記録まで行い起動コマンドを表示 (--json も同じ)。--dry-run は何も書かず計画を表示
kotoshiro plan [<text>|-] [--todo <id>] [--children] [--replace]
                            # 箇条書き (- / * / 1.) の各行を checklist に足す。--children は子 TODO として起票 (repos は親から継承)
                            # --replace は未消化の checklist を置き換える。inbox なら planning に進める。journal に kind=plan
kotoshiro note <text>|-     # journal 追記 (セッションが skill 経由で打つ)
kotoshiro check <n> [--undo]  # checklist の n 番目をトグル。journal に自動記録
kotoshiro status <id> <st>  # inbox|planning|active|done。done は checklist 全消化 + blocked_by 全 done が条件 (--force で無視)
kotoshiro hook stop         # Stop hook。last_assistant_message を log/ に追記
kotoshiro hook session-end  # SessionEnd hook。ended_at 記録、digest を detach 起動
kotoshiro digest <id>       # digest の (再) 生成
kotoshiro list [--all] / show <id>
                            # list は一行 digest、show は 3 項目 digest + checklist + sessions (herdr なら agent の状態も) + journal 末尾
kotoshiro tree [<id>] [--all]
                            # parent で木を組んで表示。done は --all の時だけ (子に未完了があれば親は出る)
kotoshiro issue create <id> [--repo org/repo] [--yes]
                            # title + 本文で gh issue create し、できた issue を link する。repo は TODO の repos が 1 つならそれ。本文を見せて確認
kotoshiro done <id> [--force] [--yes] [--no-comment] [--keep-worktree] [--dry-run]
                            # 条件: checklist 全消化 + blocked_by 全 done + live session なし + repo ごとに merged PR があり open PR がない
                            # PR は gh pr list --head todo/<id> --state all で引いて prs に記録。--force は条件違反と gh 失敗を警告に落とす
                            # issue があれば journal から haiku で完了コメントを生成 → 表示 → 確認 (--yes で省略) → gh issue comment を 1 回
                            # 片付け: git worktree remove + branch -d (--keep-worktree で残す)、herdr なら workspace close。issue close は手動
                            # --dry-run は条件判定と PR 収集だけ行い、何も書かない

# 今後
kotoshiro wait <id>         # driver セッション用。Runtime::wait を足してから
```

## 同梱 skill (セッションに読ませる)

1. `kotoshiro show --json` で現状を読める
2. 区切りごとに `kotoshiro note`: checklist 1 項目完了 / 詰まった / ユーザーに返す前
3. 計画が固まったら `kotoshiro plan -` に箇条書きを流す (checklist になる。note は不要)。checklist は `kotoshiro check <n>` で消す
4. スコープ外の障害は `kotoshiro new --blocks $KOTOSHIRO_TODO`
5. `kotoshiro status ... done` は打たない。完了判断は人

## 作業ストーリーの確認済み経路

- 3a 障害 issue 発見: `todo new --blocks` → 現 TODO は blocked。解消後の再開は session resume ではなく journal を prompt に渡す新セッション (日が空いた TODO に戻る経路と同一)
- 3b 複数リポ: `repos: []`、リポごとに worktree + pane を同一 workspace の別 tab。pane token `todo=<id> repo=<name>`。契約は journal の contract セクション
- Epic: `parent_id`。自走は driver セッションが `todo tree` を読み `todo start` を打ち、`agent wait --until blocked` で詰まりを拾う。`events.subscribe` で push 受信

## 実装方針

- 言語: Rust。単一バイナリ、小さく、起動が速いこと (Stop hook から毎回起動される) が理由。herdr と言語が揃う副次効果あり
  - 不採用: TypeScript + Bun compile (Claude Code 自身はこの構成だが 60MB 超のバイナリを hook から毎回起動することになる)、Node + npm (利用者に Node を要求)
  - Claude Code のランタイム相乗りは不可。Bun compile の自己完結バイナリで、外部スクリプトを実行する口はない
- 外部入力の検証は 2 段。kotoshiro が形を指定して要求する JSON (`gh --json <fields>`) は `serde` + `deny_unknown_fields` で厳格に。フィールドを指定できない出力 (Claude Code の hook stdin と `claude -p`、herdr の CLI 出力) はバージョン毎にフィールドが増えるため、必要フィールドだけ取り未知は無視する
- 要約器の子プロセスは `claude -p --model haiku --setting-sources "" --strict-mcp-config --no-session-persistence` で起動する。`--bare` は OAuth を読まない仕様 (API キー専用) なので使えない。hook の再帰は env `KOTOSHIRO_NESTED=1` で止める: hook コマンドはこれを見たら即終了する。`--setting-sources ""` で設定 (hook 含む) を読まないので二重の防御になる
- CLI は `clap`。全サブコマンドに `--json` を持たせ、skill / driver セッションから機械読みできるようにする
- ストアは plain text。DB は入れない。並行書き込みは TODO ディレクトリ単位のファイルロック (Rust 1.89 で安定化した `std::fs::File::lock`。外部 crate 不要)
- 要約器は `Summarizer` port。既定実装は `claude -p --model haiku --output-format json` を子プロセスで叩く。ユーザーの Claude ログインを流用するので API キーも SDK も不要。`KOTOSHIRO_SUMMARIZER=fixed` で固定文字列を返す実装に差し替える (テスト用)
- 子プロセス (`git` / `gh` / `herdr` / `claude`) の起動は 1 モジュールに集め、テストで差し替え可能にする
- 配布: `cargo-dist` で GitHub Releases + brew tap。クロスビルド対象は macOS (arm64 / x86_64) と Linux (x86_64 / arm64)

## 最初の 1 週間

痛み 2・3 (ストアと進捗収集、digest) だけ作る。Runtime は `manual` adapter のみ。herdr で pane を開き、そこで `kotoshiro start <id>` を打つ運用。
対象: init / new / link / start / note / check / status / hook / digest / list / show。

## 週 2

herdr adapter、`plan` / `tree` / `issue create` / `done`、cargo-dist (GitHub Releases + brew tap `hachibeeDI/homebrew-tap`)。
driver セッション (Epic 自走) と `Runtime::wait` は対象外。

## 参照

- herdr CLI: https://herdr.dev/docs/cli-reference/
- herdr socket API: https://herdr.dev/docs/socket-api/
- herdr plugins (将来の pane 表示): https://herdr.dev/docs/plugins/
- Claude Code cross-session messaging: https://code.claude.com/docs/en/cross-session-messaging.md
- Claude Code hooks: https://code.claude.com/docs/en/hooks.md
- 比較対象: langwatch/kanban-code (card = session、親子カード + channel)
