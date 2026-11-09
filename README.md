# kotoshiro (事代主)

[![CI](https://github.com/hachibeeDI/Kotoshiro/actions/workflows/ci.yml/badge.svg)](https://github.com/hachibeeDI/Kotoshiro/actions/workflows/ci.yml)

TODO を根に据えて Claude Code セッション群を司る CLI。

- TODO は GitHub issue にもリポジトリにも縛られない。issue は任意、1 TODO が複数リポをまたいでよい
- 進捗の正本はセッション自身が書く journal。Stop hook は生ログを拾うだけで、LLM を回さない
- 人が読むのは digest。journal から haiku で機械生成する「一行」と「いま / 次 / 詰まり」
- 週末をまたいで戻ってきた時に `todo list` で状況が一目で分かることが目的

設計判断と制約は [DESIGN.md](DESIGN.md) にある。

## インストール

Rust 1.89 以上。

```sh
cargo install --git https://github.com/hachibeeDI/Kotoshiro
kotoshiro init                                        # Claude の設定が ~/.claude なら
kotoshiro init --claude-config-dir ~/.claude-work     # CLAUDE_CONFIG_DIR を変えているなら
```

`init` がやること (冪等。何度打ってもよい):

| 対象 | 内容 |
|---|---|
| `~/todos/` | ストアを作る。`KOTOSHIRO_HOME` で変更可 |
| `~/todos/config.toml` | 設定の雛形。`--claude-config-dir` を渡すと記録する |
| `<claude 設定 dir>/settings.json` | Stop / SessionEnd hook を追記。他の hook は触らない |
| `<claude 設定 dir>/skills/kotoshiro/SKILL.md` | セッションに読ませる作法を配置 |
| `todo` | `kotoshiro` と同じディレクトリにシンボリックリンク。衝突する時は `--no-link` |

Claude の設定 dir は `--claude-config-dir` > env `CLAUDE_CONFIG_DIR` > `config.toml` > `~/.claude` の順で決まる。一度 config に記録すれば以後は env の無い window から打っても同じ場所に書く。

### 設定 (`~/todos/config.toml`)

```toml
runtime = "herdr"                                   # manual | herdr
worktree = "{repo}/.claude/worktrees/todo-{id}"    # 既定 "{repo}/../{name}-wt/todo-{id}"
claude_config_dir = "~/.claude-work"
```

優先順はコマンドのフラグ > 環境変数 > `config.toml` > 既定。未知のキーはエラーになる。
`claude_config_dir` は `init` の書き先になるだけでなく、`start` が起動する claude と要約器の子プロセスにも `CLAUDE_CONFIG_DIR` として渡る。env の無い pane から `todo start` を打っても hook と skill とログインが揃う。

## 使い方

### 1. TODO を作る

```sh
todo new "ログイン失敗時のリトライ" --issue org/app#123 --repo ~/src/app
todo new "税金の書類"                      # issue もリポも無い単独 TODO
todo new "共通ライブラリの修正" --repo ~/src/app --repo ~/src/lib   # 複数リポ
```

ID は連番。`--repo` は作業ディレクトリのパスで、`git rev-parse --show-toplevel` で正規化し、既定ブランチ (origin/HEAD) を base として記録する。

本文と checklist は `--body` か、作成後に `~/todos/<id>/todo.md` を直接編集する。`- [ ]` 形式の行が checklist になる。

```sh
todo new "リトライ実装" --repo ~/src/app --body "## Checklist

- [ ] 失敗時に指数バックオフ
- [ ] 上限回数を設定で持つ
- [ ] テスト"
```

### 2. セッションを起動する

```sh
todo start 1              # このプロセスを claude に差し替える
todo start 1 --repo lib   # repos が複数ある時は選ぶ
todo start 1 --dry-run    # 何も書かず、何が起きるかだけ見る
todo start 1 --print      # 記録はするが起動せず、コマンドを表示する (別の場所で打つ用)
```

`start` の動作:

1. `<repo>/../<repo名>-wt/todo-<id>` に worktree を作る (ブランチ `todo/<id>`)。既にあれば再利用
2. `~/todos/<id>/context.md` を生成する。title、checklist、digest、journal 末尾が入る
3. セッション ID を採番して `sessions.json` に記録し、status を active にする
4. `claude --session-id <uuid> --name todo-<id>-<repo> --append-system-prompt-file context.md` を `KOTOSHIRO_TODO=<id>` 付きで exec する

repos が空の TODO はカレントディレクトリで起動する。`--json` は exec と両立しないので `--print` 扱い。

`--print` で表示したコマンドを結局打たなかった場合、`sessions.json` に終わらないセッションが残る (S 列に出続ける)。見るだけなら `--dry-run` を使う。

日が空いた TODO に戻る時も `start` を打つ。セッションを resume するのではなく、journal を読んだ新しいセッションを立てる。

#### herdr で起動する

```sh
# config.toml に runtime = "herdr"。または env KOTOSHIRO_RUNTIME=herdr、todo start 1 --runtime herdr
todo start 1                        # 新しい workspace を作り、その pane で claude を起動
todo start 1 --repo lib --no-focus  # 同じ workspace に tab を足す。フォーカスは移さない
```

対応は TODO = workspace (label `#1 タイトル`)、repo = tab、セッション = pane 内の agent。workspace には `KOTOSHIRO_TODO` が env として付くので、その中で手で開いた pane から起動した claude も hook が拾う。
workspace は label で毎回探すので herdr を再起動しても壊れない。`todo show` の sessions に pane id と agent の状態 (`[working]` / `[blocked]` / `[idle]`) が出る。

herdr 以外では、pane を開いてそこで `todo start <id>` を打つ (manual)。

### 3. セッション内で起きること

セッションには skill `kotoshiro` が入っている。Claude は次を自分で打つ:

```sh
kotoshiro plan - <<'EOF'            # 計画が固まったら箇条書きを流す → checklist になる
- 失敗時に指数バックオフ
- 上限回数を設定で持つ
- テスト
EOF
kotoshiro note "バックオフ実装、テスト通過。設定は config.toml に足す"
kotoshiro check 1
kotoshiro new "lib 側の timeout が固定値" --blocks $KOTOSHIRO_TODO   # スコープ外の障害
```

`plan` は行頭の箇条書き (`- ` / `* ` / `1. `) を拾い、inbox なら planning に進める。`--children` で子 TODO として起票、`--replace` で未消化分を置き換える。
`note` は区切りごと (checklist 1 項目完了、詰まった、ユーザーに返す前)。`--blocks` で作った TODO が未完了の間、元の TODO は blocked と表示される。

hook は自動で動く:

- Stop (ターン毎): Claude の最後の発話を `~/todos/<id>/log/<session>.md` に追記する。LLM は呼ばない
- SessionEnd: `ended_at` を記録し、digest 生成をバックグラウンドで起動する。Claude の終了は待たせない

セッションと TODO の紐付けは `--session-id` → `KOTOSHIRO_TODO` → cwd が worktree 配下か、の順で解決する。`start` を使わず手で `KOTOSHIRO_TODO=1 claude` と起動しても拾える。

### 4. 状況を見る

```
$ todo list
ID  STATUS   B TITLE                  AGE  S  DIGEST
 1  active     リトライ実装            2h  1  バックオフ実装済み。次は上限回数の設定化
 2  inbox    B 共通ライブラリの修正    1d  0  ~lib の timeout 調査中。#3 待ち
 3  active     lib 側の timeout が固定値 30m  1  -
```

| 列 | 意味 |
|---|---|
| B | blocked (blocked_by に未完了 TODO がある) |
| AGE | 最後の更新からの経過 |
| S | 動作中セッション数 |
| DIGEST | 一行 digest。`~` 前置は journal より古い (stale) |

```
$ todo show 1
#1 リトライ実装
status: active   updated: 2026-11-06 00:16 (2h ago)
repos: app (/Users/me/src/app, base main)

## digest (2026-11-06 00:10)
バックオフ実装済み。次は上限回数の設定化

いま: retry.rs に指数バックオフを実装し単体テストが通った
次: 上限回数を config.toml から読む
詰まり: なし

## checklist
 1. [x] 失敗時に指数バックオフ
 2. [ ] 上限回数を設定で持つ
 3. [ ] テスト

## sessions
* 3f9a1c2e 2026-11-06 00:16 repo=app

## journal (tail)
- 2026-11-06 00:20 [note 3f9a1c2e] バックオフ実装、テスト通過。次は設定化
```

digest が stale なら `todo digest 1` で再生成する。生成に失敗しても前回の digest は残り、警告だけ出る。

Epic (`--parent`) で分けた TODO は `todo tree` で木として見る。

```
$ todo tree
#1 planning    認証まわり刷新  ~設計確定。#2 から着手
├─ #2 active     トークン更新  リフレッシュ実装中
└─ #3 inbox    B 監査ログ
```

### 5. 終わらせる

```sh
todo done 1 --dry-run   # 条件判定と PR 収集だけ
todo done 1
```

`done` がやること:

1. 条件判定: checklist 全消化、blocked_by 全 done、動作中セッションなし、repo ごとに merged PR があり open PR がない (`gh pr list --head todo/<id>` で引く)。違反があれば止まる。`--force` で警告に落とす
2. issue があれば journal から haiku で完了コメントを生成して表示し、確認のうえ `gh issue comment` を 1 回打つ。`--yes` で確認省略、`--no-comment` で投稿しない
3. worktree を外してブランチを消す (`--keep-worktree` で残す)。herdr なら workspace を閉じる
4. status を done にし、PR 一覧を `prs` に記録する

issue を閉じるのは人がやる。issue が無い TODO は `todo issue create <id>` で起こして link できる (repo は TODO の repos が 1 つならそれ、`--repo owner/repo` で指定)。

`todo status 1 done` は片付けを伴わない完了 (条件は checklist と blocked_by のみ)。セッションは `status done` も `done` も打たない。完了判断は人がする。

## コマンド一覧

```
kotoshiro init [--no-link] [--claude-config-dir PATH]
kotoshiro new <title> [--issue org/repo#N] [--repo PATH]... [--blocks ID] [--parent ID] [--body TEXT|-]
kotoshiro link <id> --issue org/repo#N
kotoshiro start <id> [--repo NAME] [--print] [--dry-run] [--runtime manual|herdr] [--no-focus]
kotoshiro plan [TEXT|-] [--todo ID] [--children] [--replace]
kotoshiro note [TEXT|-] [--todo ID]
kotoshiro check <n> [--undo] [--todo ID]
kotoshiro status <id> <inbox|planning|active|done> [--force]
kotoshiro done <id> [--force] [--yes] [--no-comment] [--keep-worktree] [--dry-run]
kotoshiro issue create <id> [--repo owner/repo] [--yes]
kotoshiro digest <id> [--quiet]
kotoshiro list [--all]
kotoshiro show [id] [--journal N]
kotoshiro tree [id] [--all]
kotoshiro hook <stop|session-end>        # Claude Code から呼ばれる。stdin に JSON
```

全コマンドに `--json`。skill や他のセッションから機械読みする用。

## 環境変数

| 変数 | 意味 |
|---|---|
| `KOTOSHIRO_HOME` | ストアの場所。既定 `~/todos` |
| `KOTOSHIRO_TODO` | 現在の TODO ID。`start` が起動コマンドに付ける |
| `KOTOSHIRO_SESSION` | 現在のセッション ID。journal の記録者 |
| `KOTOSHIRO_RUNTIME` | セッション起動の adapter。`manual` (既定) / `herdr`。config の `runtime` より優先 |
| `KOTOSHIRO_SUMMARIZER` | `claude` (既定) / `fixed` / `fixed:<text>`。テスト用 |
| `KOTOSHIRO_SUMMARIZER_MODEL` | 要約に使うモデル。既定 `haiku` |
| `KOTOSHIRO_NESTED` | 要約器の子プロセスに付く。hook はこれを見たら即終了する |
| `CLAUDE_CONFIG_DIR` | Claude Code の設定 dir。config の `claude_config_dir` より優先 |

要約は `claude -p --model haiku` を子プロセスで叩く。ユーザーの Claude ログインを流用するので API キーは要らない。

## ストア

```
~/todos/
  config.toml             # 設定
  .counter                # 次の ID
  <id>/
    todo.md               # YAML frontmatter + 本文。checklist は本文中の - [ ] 行
    journal.md            # 進捗の正本。セッションが note / check で書く
    log/<session>.md      # Stop hook の生ログ
    digest.md             # journal + log から生成。手で編集しない
    sessions.json         # セッション記録。herdr なら pane id も
    context.md            # start が生成する system prompt 追記分
```

全部 plain text。`todo.md` の本文と checklist は手で編集してよい。frontmatter の `prs` は `done` が書く。

## 開発

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

統合テストは `KOTOSHIRO_SUMMARIZER=fixed` で haiku を呼ばずに動く。herdr adapter は `tests/week2.rs` がフェイクの `herdr` スクリプトを PATH に置いて検証する。
