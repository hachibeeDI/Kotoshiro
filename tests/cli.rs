use std::path::Path;
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
            .env_remove("KOTOSHIRO_SESSION");
        c
    }

    fn todo(&self, id: u64) -> std::path::PathBuf {
        self.home.path().join(id.to_string())
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

#[test]
fn new_list_show_roundtrip() {
    let e = Env::new();
    e.cmd()
        .args(["new", "最初の TODO", "--issue", "org/repo#1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("created #1"));
    e.cmd()
        .args(["new", "二つ目"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#2"));

    e.cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("最初の TODO").and(predicate::str::contains("inbox")));

    let out = e.cmd().args(["--json", "show", "1"]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["id"], 1);
    assert_eq!(v["issue"], "org/repo#1");
    assert_eq!(v["blocked"], false);
}

#[test]
fn note_check_status_flow() {
    let e = Env::new();
    e.cmd()
        .args([
            "new",
            "作業",
            "--body",
            "説明\n\n## Checklist\n\n- [ ] a\n- [ ] b",
        ])
        .assert()
        .success();

    // env で現在 TODO を解決
    e.cmd()
        .env("KOTOSHIRO_TODO", "1")
        .env("KOTOSHIRO_SESSION", "sess-0001-xxxx")
        .args(["note", "計画を立てた"])
        .assert()
        .success()
        .stdout(predicate::str::contains("noted #1"));
    let j = read(&e.todo(1).join("journal.md"));
    assert!(j.contains("kind=note"));
    assert!(j.contains("session=sess-000"));
    assert!(j.contains("計画を立てた"));

    e.cmd()
        .args(["check", "1", "--todo", "1"])
        .assert()
        .success();
    e.cmd()
        .args(["status", "1", "done"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("1 checklist item(s) left"));
    e.cmd()
        .args(["check", "2", "--todo", "1"])
        .assert()
        .success();
    e.cmd().args(["status", "1", "done"]).assert().success();
    e.cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no todos)"));
    e.cmd()
        .args(["list", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("done"));

    // 現在 TODO が解決できなければエラー
    e.cmd()
        .args(["note", "x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no current TODO"));
}

#[test]
fn blocks_and_derived_blocked() {
    let e = Env::new();
    e.cmd().args(["new", "本体"]).assert().success();
    e.cmd()
        .args(["new", "障害", "--blocks", "1"])
        .assert()
        .success();

    let out = e.cmd().args(["--json", "list"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["id"], 1);
    assert_eq!(v[0]["blocked"], true);
    assert_eq!(v[1]["blocked"], false);

    e.cmd()
        .args(["status", "1", "done"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("blocked by #2"));
    e.cmd().args(["status", "2", "done"]).assert().success();
    e.cmd().args(["status", "1", "done"]).assert().success();

    // 存在しない TODO は block できない
    e.cmd()
        .args(["new", "c", "--blocks", "99"])
        .assert()
        .failure();
}

#[test]
fn hook_stop_logs_and_digest() {
    let e = Env::new();
    e.cmd().args(["new", "hook 対象"]).assert().success();

    let payload = serde_json::json!({
        "session_id": "abcd1234-0000",
        "cwd": "/nowhere",
        "hook_event_name": "Stop",
        "stop_hook_active": false,
        "last_assistant_message": "実装を終えました。次はテスト。",
        "background_tasks": [],
    })
    .to_string();

    // 紐付け無し → 何もしない
    e.cmd()
        .args(["hook", "stop"])
        .write_stdin(payload.clone())
        .assert()
        .success();
    assert!(!e.todo(1).join("log").exists());

    // env で紐付け → log 追記 + session 登録
    e.cmd()
        .env("KOTOSHIRO_TODO", "1")
        .args(["hook", "stop"])
        .write_stdin(payload.clone())
        .assert()
        .success();
    let log = read(&e.todo(1).join("log").join("abcd1234-0000.md"));
    assert!(log.contains("実装を終えました"));
    let sessions = read(&e.todo(1).join("sessions.json"));
    assert!(sessions.contains("abcd1234-0000"));

    // 2 回目は session_id 照合で紐付く (env なし)
    e.cmd()
        .args(["hook", "stop"])
        .write_stdin(payload)
        .assert()
        .success();
    let log = read(&e.todo(1).join("log").join("abcd1234-0000.md"));
    assert_eq!(log.matches("実装を終えました").count(), 2);

    // digest (fixed summarizer)
    e.cmd()
        .args(["digest", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("digest updated"));
    let d = read(&e.todo(1).join("digest.md"));
    assert!(d.contains("## いま"));
    e.cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(fixed)"));

    // note で stale に
    e.cmd()
        .env("KOTOSHIRO_TODO", "1")
        .args(["note", "追記"])
        .assert()
        .success();
    e.cmd()
        .args(["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("~(fixed)"));

    // session-end で ended_at、digest detach
    let end =
        serde_json::json!({"session_id": "abcd1234-0000", "cwd": "/nowhere", "reason": "other"})
            .to_string();
    e.cmd()
        .args(["hook", "session-end"])
        .write_stdin(end)
        .assert()
        .success();
    let sessions = read(&e.todo(1).join("sessions.json"));
    assert!(sessions.contains("ended_at"));

    // nested guard
    e.cmd()
        .env("KOTOSHIRO_NESTED", "1")
        .env("KOTOSHIRO_TODO", "1")
        .args(["hook", "stop"])
        .write_stdin("not json")
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn bad_summarizer_output_keeps_old_digest() {
    let e = Env::new();
    e.cmd().args(["new", "x"]).assert().success();
    e.cmd().args(["digest", "1"]).assert().success();
    let before = read(&e.todo(1).join("digest.md"));
    e.cmd()
        .env("KOTOSHIRO_SUMMARIZER", "fixed:garbage without labels")
        .args(["digest", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not updated"));
    e.cmd()
        .env("KOTOSHIRO_SUMMARIZER", "fixed:garbage without labels")
        .args(["digest", "1", "--quiet"])
        .assert()
        .success();
    assert_eq!(before, read(&e.todo(1).join("digest.md")));
}

#[test]
fn start_with_repo_creates_worktree() {
    let e = Env::new();
    let work_dir = tempfile::tempdir().unwrap();
    // macOS の /var は /private/var への symlink。git が返す正規パスと揃える
    let work = work_dir.path().canonicalize().unwrap();
    let repo = work.join("myrepo");
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

    e.cmd()
        .args(["new", "repo 作業", "--repo", repo.to_str().unwrap()])
        .assert()
        .success();
    let out = e.cmd().args(["--json", "start", "1"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let wt = work.join("myrepo-wt").join("todo-1");
    assert_eq!(v["worktree"], wt.to_str().unwrap());
    assert!(wt.join("README").exists());
    let cmd = v["command"].as_str().unwrap();
    assert!(cmd.contains("KOTOSHIRO_TODO=1"));
    assert!(cmd.contains("--session-id"));
    assert!(cmd.contains("--append-system-prompt-file"));
    assert!(e.todo(1).join("context.md").exists());

    // status が active に、session が live
    let show = e.cmd().args(["--json", "show", "1"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(v["status"], "active");
    assert_eq!(v["sessions"].as_array().unwrap().len(), 1);

    // cwd 照合: worktree 内から hook が来たら紐付く
    let sid = v["sessions"][0]["session_id"].as_str().unwrap().to_owned();
    let payload = serde_json::json!({
        "session_id": "other-session", "cwd": wt.to_str().unwrap(), "last_assistant_message": "hi"
    })
    .to_string();
    e.cmd()
        .args(["hook", "stop"])
        .write_stdin(payload)
        .assert()
        .success();
    assert!(e.todo(1).join("log").join("other-session.md").exists());
    let _ = sid;

    // 2 回目の start は worktree を再利用
    e.cmd().args(["start", "1"]).assert().success();
}

#[test]
fn init_writes_hooks_and_skill() {
    let e = Env::new();
    let cfg = tempfile::tempdir().unwrap();
    std::fs::write(
        cfg.path().join("settings.json"),
        r#"{"permissions":{"allow":["Bash"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"other"}]}]}}"#,
    )
    .unwrap();
    e.cmd()
        .env("CLAUDE_CONFIG_DIR", cfg.path())
        .args(["init", "--no-link"])
        .assert()
        .success();
    let s = read(&cfg.path().join("settings.json"));
    assert!(s.contains("hook stop"));
    assert!(s.contains("hook session-end"));
    assert!(s.contains("\"other\""));
    assert!(s.contains("Bash"));
    assert!(cfg.path().join("skills/kotoshiro/SKILL.md").exists());
    assert!(e.home.path().is_dir());

    // 2 回目は無変更
    let out = e
        .cmd()
        .env("CLAUDE_CONFIG_DIR", cfg.path())
        .args(["--json", "init", "--no-link"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["hooks_changed"], false);
    assert_eq!(v["skill_changed"], false);
}
