//! 週 2: plan / tree / done / herdr adapter (フェイク herdr)。

use std::path::{Path, PathBuf};
use std::process::Command;

use predicates::prelude::*;
use tempfile::TempDir;

struct Env {
    home: TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
        }
    }

    fn cmd(&self) -> assert_cmd::Command {
        let mut c = assert_cmd::Command::cargo_bin("kotoshiro").unwrap();
        c.env("KOTOSHIRO_HOME", self.home.path())
            .env("KOTOSHIRO_SUMMARIZER", "fixed")
            .env_remove("KOTOSHIRO_TODO")
            .env_remove("KOTOSHIRO_SESSION")
            .env_remove("KOTOSHIRO_RUNTIME")
            // 開発者のシェルの env が config の検証に混ざらないように
            .env_remove("CLAUDE_CONFIG_DIR");
        c
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        let out = self.cmd().arg("--json").args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn todo(&self, id: u64) -> PathBuf {
        self.home.path().join(id.to_string())
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// 一時 git リポジトリ。macOS の /var symlink を解決した実パスを返す。
fn git_repo() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap().join("myrepo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let st = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README"), "x").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "init"]);
    (dir, repo)
}

#[test]
fn plan_fills_checklist_and_children_show_in_tree() {
    let e = Env::new();
    e.cmd().args(["new", "Epic"]).assert().success();

    // 行頭の箇条書きだけ。入れ子は無視
    e.cmd()
        .args(["plan", "--todo", "1"])
        .write_stdin("計画:\n- one\n  - nested\n* two\n1. three\n")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("3 item(s) added")
                .and(predicate::str::contains(" 3. [ ] three")),
        );
    let v = e.json(&["show", "1"]);
    assert_eq!(v["status"], "planning");
    assert_eq!(v["checklist"].as_array().unwrap().len(), 3);
    assert!(read(&e.todo(1).join("journal.md")).contains("kind=plan"));

    // --replace は未消化だけ置き換える
    e.cmd()
        .args(["check", "1", "--todo", "1"])
        .assert()
        .success();
    e.cmd()
        .args(["plan", "--todo", "1", "--replace", "- only"])
        .assert()
        .success();
    let v = e.json(&["show", "1"]);
    let items = v["checklist"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["text"], "one");
    assert_eq!(items[0]["done"], true);
    assert_eq!(items[1]["text"], "only");

    // 子 TODO
    e.cmd()
        .args(["plan", "--todo", "1", "--children", "- child a\n- child b"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#2 child a").and(predicate::str::contains("#3 child b")));
    assert_eq!(e.json(&["show", "2"])["parent"], 1);

    e.cmd().args(["tree"]).assert().success().stdout(
        predicate::str::contains("#1 planning")
            .and(predicate::str::contains("├─ #2 inbox"))
            .and(predicate::str::contains("└─ #3 inbox")),
    );
    let t = e.json(&["tree"]);
    assert_eq!(t.as_array().unwrap().len(), 1);
    assert_eq!(t[0]["children"].as_array().unwrap().len(), 2);

    // done は --all の時だけ出る
    e.cmd().args(["status", "2", "done"]).assert().success();
    e.cmd()
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#2").not());
    e.cmd()
        .args(["tree", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#2 done"));
    // 根を指定すればその木だけ
    let t = e.json(&["tree", "3"]);
    assert_eq!(t[0]["id"], 3);

    e.cmd()
        .args(["plan", "--todo", "1", "just prose"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no list items"));
}

#[test]
fn done_checks_conditions_and_records() {
    let e = Env::new();
    e.cmd()
        .args(["new", "単独", "--body", "## Checklist\n\n- [ ] a"])
        .assert()
        .success();

    // dry-run は書かずに判定だけ
    let v = e.json(&["done", "1", "--dry-run"]);
    assert_eq!(v["ok"], false);
    assert!(v["problems"][0]
        .as_str()
        .unwrap()
        .contains("1 checklist item(s) left"));
    assert_eq!(e.json(&["show", "1"])["status"], "inbox");

    e.cmd()
        .args(["done", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot finish #1"));
    e.cmd()
        .args(["check", "1", "--todo", "1"])
        .assert()
        .success();

    // live session があると拒否、--force で通る
    e.json(&["start", "1"]);
    e.cmd()
        .args(["done", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("live session"));
    e.cmd()
        .args(["done", "1", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#1 done"));
    let v = e.json(&["show", "1"]);
    assert_eq!(v["status"], "done");
    let j = read(&e.todo(1).join("journal.md"));
    assert!(j.contains("kind=done"));
    assert!(j.contains("active -> done"));

    e.cmd()
        .args(["done", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already done"));
}

#[test]
fn done_removes_worktree() {
    let e = Env::new();
    let (_dir, repo) = git_repo();
    e.cmd()
        .args(["new", "repo 作業", "--repo", repo.to_str().unwrap()])
        .assert()
        .success();
    let v = e.json(&["start", "1"]);
    let wt = PathBuf::from(v["worktree"].as_str().unwrap());
    assert!(wt.join("README").exists());

    // gh が無い / リモートが無いので PR 確認は失敗 → --force で警告に落ちる
    e.cmd()
        .args(["done", "1", "--force", "--no-comment"])
        .assert()
        .success()
        .stdout(predicate::str::contains("worktree removed"));
    assert!(!wt.exists());
    assert_eq!(e.json(&["show", "1"])["status"], "done");

    // --keep-worktree
    e.cmd()
        .args(["new", "二つ目", "--repo", repo.to_str().unwrap()])
        .assert()
        .success();
    let v = e.json(&["start", "2"]);
    let wt2 = PathBuf::from(v["worktree"].as_str().unwrap());
    e.cmd()
        .args(["done", "2", "--force", "--no-comment", "--keep-worktree"])
        .assert()
        .success();
    assert!(wt2.exists());
}

#[cfg(unix)]
fn fake_herdr(dir: &Path, log: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        r##"#!/bin/sh
LOG="{log}"
echo "$@" >> "$LOG"
if [ -n "$FAKE_FAIL" ] && [ "$1 $2" = "agent start" ]; then
  echo '{{"error":{{"message":"agent_not_ready"}}}}' >&2; exit 1
fi
case "$1 $2" in
  "workspace list")
    if [ -f "$LOG.ws" ]; then
      echo '{{"id":"x","result":{{"type":"workspace_list","workspaces":[{{"workspace_id":"w9","label":"#99 other"}},{{"workspace_id":"w3","label":"#1 herdr 対象","number":3}}]}}}}'
    else
      echo '{{"id":"x","result":{{"type":"workspace_list","workspaces":[{{"workspace_id":"w9","label":"#99 other"}}]}}}}'
    fi ;;
  "workspace create")
    touch "$LOG.ws"
    echo '{{"id":"x","result":{{"workspace":{{"workspace_id":"w3"}},"tab":{{"tab_id":"w3:t1"}},"root_pane":{{"pane_id":"w3:p1","extra":1}}}}}}' ;;
  "tab create")
    echo '{{"id":"x","result":{{"tab":{{"tab_id":"w3:t2"}},"root_pane":{{"pane_id":"w3:p2"}}}}}}' ;;
  "agent start")
    echo '{{"id":"x","result":{{"type":"agent_started"}}}}' ;;
  "agent get")
    echo '{{"id":"x","result":{{"agent":{{"agent_status":"working","pane_id":"w3:p1"}}}}}}' ;;
  "workspace close")
    echo '{{"id":"x","result":{{}}}}' ;;
  *)
    echo '{{"error":{{"message":"unknown command"}}}}' >&2; exit 1 ;;
esac
"##,
        log = log.display()
    );
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let p = bin.join("herdr");
    std::fs::write(&p, script).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

#[cfg(unix)]
#[test]
fn herdr_adapter_creates_workspace_then_tab_and_closes() {
    let e = Env::new();
    let tmp = tempfile::tempdir().unwrap();
    let log = tmp.path().join("herdr.log");
    let bin = fake_herdr(tmp.path(), &log);
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let herdr = |e: &Env| {
        let mut c = e.cmd();
        c.env("PATH", &path).env("KOTOSHIRO_RUNTIME", "herdr");
        c
    };

    e.cmd().args(["new", "herdr 対象"]).assert().success();

    // --dry-run は herdr を呼ばない
    herdr(&e)
        .args(["start", "1", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("runtime:   herdr"));
    assert!(!log.exists());

    // 1 回目: workspace が無いので作り、root pane に agent start
    herdr(&e)
        .args(["start", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#1 started: herdr w3:p1"));
    let l = read(&log);
    assert!(l.contains("workspace list"));
    assert!(l.contains("workspace create --cwd "));
    assert!(
        l.contains("--label #1 herdr 対象 --focus --env KOTOSHIRO_TODO=1 --env KOTOSHIRO_SESSION=")
    );
    assert!(l.contains("agent start todo-1-"));
    assert!(l.contains("--kind claude --pane w3:p1 -- --session-id "));
    assert!(l.contains("--append-system-prompt-file"));
    let out = herdr(&e).args(["--json", "show", "1"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let s = &v["sessions"][0];
    assert_eq!(s["runtime"]["runtime"], "herdr");
    assert_eq!(s["runtime"]["ref"], "w3:p1");
    assert_eq!(s["state"], "working");
    herdr(&e)
        .args(["show", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("herdr=w3:p1 [working]"));

    // 2 回目: workspace があるので tab を足す。--no-focus
    herdr(&e)
        .args(["start", "1", "--no-focus"])
        .assert()
        .success()
        .stdout(predicate::str::contains("herdr w3:p2"));
    let l = read(&log);
    assert!(l.contains("tab create --workspace w3 --cwd "));
    assert!(l.contains("--label todo-1 --no-focus"));

    // agent start 失敗: セッション記録は閉じられる
    herdr(&e)
        .env("FAKE_FAIL", "1")
        .args(["start", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("agent_not_ready"));
    let out = herdr(&e).args(["--json", "show", "1"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let sessions = v["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 3);
    assert!(
        sessions[0]["ended_at"].is_string(),
        "failed start must not stay live"
    );

    // --print はどの runtime でも manual 形式のコマンド
    herdr(&e)
        .args(["start", "1", "--print"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("cd "));

    // done で workspace close
    herdr(&e)
        .args(["done", "1", "--force", "--no-comment"])
        .assert()
        .success()
        .stdout(predicate::str::contains("workspace closed"));
    assert!(read(&log).contains("workspace close w3"));
}

#[test]
fn unknown_runtime_is_an_error() {
    let e = Env::new();
    e.cmd().args(["new", "x"]).assert().success();
    e.cmd()
        .env("KOTOSHIRO_RUNTIME", "tmux")
        .args(["start", "1", "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not implemented"));
    e.cmd()
        .args(["start", "1", "--dry-run", "--runtime", "manual"])
        .assert()
        .success();
}

#[test]
fn config_file_sets_runtime_worktree_and_claude_dir() {
    let e = Env::new();
    let (_dir, repo) = git_repo();
    e.cmd()
        .args(["new", "設定", "--repo", repo.to_str().unwrap()])
        .assert()
        .success();
    std::fs::write(
        e.home.path().join("config.toml"),
        "runtime = \"herdr\"\nworktree = \"{repo}/.wt/todo-{id}\"\nclaude_config_dir = \"/cc/work\"\n",
    )
    .unwrap();

    let v = e.json(&["start", "1", "--dry-run"]);
    assert_eq!(v["runtime"], "herdr");
    assert_eq!(
        v["worktree"],
        repo.join(".wt").join("todo-1").to_str().unwrap()
    );
    assert!(v["command"]
        .as_str()
        .unwrap()
        .contains("CLAUDE_CONFIG_DIR=/cc/work"));

    // env はファイルに勝ち、フラグは env に勝つ
    let out = e
        .cmd()
        .env("KOTOSHIRO_RUNTIME", "manual")
        .env("CLAUDE_CONFIG_DIR", "/cc/env")
        .args(["--json", "start", "1", "--dry-run"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["runtime"], "manual");
    assert!(v["command"]
        .as_str()
        .unwrap()
        .contains("CLAUDE_CONFIG_DIR=/cc/env"));
    let out = e
        .cmd()
        .env("KOTOSHIRO_RUNTIME", "manual")
        .args(["--json", "start", "1", "--dry-run", "--runtime", "herdr"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["runtime"], "herdr");

    // 壊れた config は止まる (hook 以外)
    std::fs::write(e.home.path().join("config.toml"), "runtim = \"x\"\n").unwrap();
    e.cmd()
        .args(["list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("config.toml"));
    e.cmd()
        .args(["hook", "stop"])
        .write_stdin("{}")
        .assert()
        .success()
        .stderr(predicate::str::contains("using defaults"));
}

#[test]
fn init_writes_config_and_records_claude_dir() {
    let e = Env::new();
    let cfg = tempfile::tempdir().unwrap();
    e.cmd()
        .env_remove("CLAUDE_CONFIG_DIR")
        .args(["init", "--no-link"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config:"));
    let config = e.home.path().join("config.toml");
    let s = read(&config);
    assert!(
        s.contains("# claude_config_dir"),
        "default dir is not recorded"
    );
    assert!(s.contains("# runtime = \"manual\""));

    // --claude-config-dir は記録され、コメントは残る
    let v = e.json(&[
        "init",
        "--no-link",
        "--claude-config-dir",
        cfg.path().to_str().unwrap(),
    ]);
    assert_eq!(v["config_changed"], true);
    assert_eq!(v["claude_config_dir"], cfg.path().to_str().unwrap());
    assert!(cfg.path().join("skills/kotoshiro/SKILL.md").exists());
    let s = read(&config);
    assert!(s.contains("# runtime = \"manual\""));
    assert!(s.contains(&format!("claude_config_dir = \"{}\"", cfg.path().display())));

    // 以後は env なしでも config の dir に書く
    let v = e.json(&["init", "--no-link"]);
    assert_eq!(v["config_changed"], false);
    assert_eq!(v["claude_config_dir"], cfg.path().to_str().unwrap());
}
