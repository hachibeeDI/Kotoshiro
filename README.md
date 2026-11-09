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
kotoshiro init
```

`init` がやること (冪等。何度打ってもよい):

| 対象 | 内容 |
|---|---|
| `~/todos/` | ストアを作る。`KOTOSHIRO_HOME` で変更可 |
| `$CLAUDE_CONFIG_DIR/settings.json` | Stop / SessionEnd hook を追記。他の hook は触らない |
| `$CLAUDE_CONFIG_DIR/skills/kotoshiro/SKILL.md` | セッションに読ませる作法を配置 |
| `todo` | `kotoshiro` と同じディレクトリにシンボリックリンク。衝突する時は `--no-link` |

`CLAUDE_CONFIG_DIR` 未設定なら `~/.claude`。

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

herdr や tmux で pane を開き、そこで `todo start <id>` を打つのが想定運用。日が空いた TODO に戻る時も同じ。セッションを resume するのではなく、journal を読んだ新しいセッションを立てる。

### 3. セッション内で起きること

セッションには skill `kotoshiro` が入っている。Claude は次を自分で打つ:

```sh
kotoshiro note "計画: バックオフ → 上限 → テストの順。設定は config.toml に足す"
kotoshiro check 1
kotoshiro new "lib 側の timeout が固定値" --blocks $KOTOSHIRO_TODO   # スコープ外の障害
```

`note` は区切りごと (計画確定、checklist 1 項目完了、詰まった、ユーザーに返す前)。`--blocks` で作った TODO が未完了の間、元の TODO は blocked と表示される。

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

### 5. 終わらせる

```sh
todo status 1 done
```

checklist が全部消えていて、blocked_by が全部 done でないと拒否される (`--force` で無視)。完了判断は人がする。セッションは `status done` を打たない。

issue へのコメント投稿、PR の merged 確認、worktree の片付けは未実装 (`todo done` として予定)。

## コマンド一覧

```
kotoshiro init [--no-link]
kotoshiro new <title> [--issue org/repo#N] [--repo PATH]... [--blocks ID] [--parent ID] [--body TEXT|-]
kotoshiro link <id> --issue org/repo#N
kotoshiro start <id> [--repo NAME] [--print] [--dry-run]
kotoshiro note [TEXT|-] [--todo ID]
kotoshiro check <n> [--undo] [--todo ID]
kotoshiro status <id> <inbox|planning|active|done> [--force]
kotoshiro digest <id> [--quiet]
kotoshiro list [--all]
kotoshiro show [id] [--journal N]
kotoshiro hook <stop|session-end>        # Claude Code から呼ばれる。stdin に JSON
```

全コマンドに `--json`。skill や他のセッションから機械読みする用。

## 環境変数

| 変数 | 意味 |
|---|---|
| `KOTOSHIRO_HOME` | ストアの場所。既定 `~/todos` |
| `KOTOSHIRO_TODO` | 現在の TODO ID。`start` が起動コマンドに付ける |
| `KOTOSHIRO_SESSION` | 現在のセッション ID。journal の記録者 |
| `KOTOSHIRO_RUNTIME` | セッション起動の adapter。現状 `manual` のみ |
| `KOTOSHIRO_SUMMARIZER` | `claude` (既定) / `fixed` / `fixed:<text>`。テスト用 |
| `KOTOSHIRO_SUMMARIZER_MODEL` | 要約に使うモデル。既定 `haiku` |
| `KOTOSHIRO_NESTED` | 要約器の子プロセスに付く。hook はこれを見たら即終了する |
| `CLAUDE_CONFIG_DIR` | `init` が hook と skill を書く先。既定 `~/.claude` |

要約は `claude -p --model haiku` を子プロセスで叩く。ユーザーの Claude ログインを流用するので API キーは要らない。

## ストア

```
~/todos/
  .counter                # 次の ID
  <id>/
    todo.md               # YAML frontmatter + 本文。checklist は本文中の - [ ] 行
    journal.md            # 進捗の正本。セッションが note / check で書く
    log/<session>.md      # Stop hook の生ログ
    digest.md             # journal + log から生成。手で編集しない
    sessions.json         # セッション記録
    context.md            # start が生成する system prompt 追記分
```

全部 plain text。`todo.md` の本文と checklist は手で編集してよい。

## 開発

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

統合テストは `KOTOSHIRO_SUMMARIZER=fixed` で haiku を呼ばずに動く。
