use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::proc::Cmd;

fn git(repo: &Path) -> Cmd {
    Cmd::new("git").cwd(repo)
}

pub fn toplevel(path: &Path) -> Result<PathBuf> {
    let out = git(path)
        .args(["rev-parse", "--show-toplevel"])
        .ok()
        .with_context(|| format!("{} is not a git repository", path.display()))?;
    Ok(PathBuf::from(out.stdout.trim()))
}

/// origin/HEAD があればそれ、無ければ現在のブランチ。
pub fn default_branch(repo: &Path) -> Result<String> {
    if let Ok(out) = git(repo)
        .args(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .ok()
    {
        let s = out.stdout.trim();
        return Ok(s.strip_prefix("origin/").unwrap_or(s).to_owned());
    }
    let out = git(repo).args(["rev-parse", "--abbrev-ref", "HEAD"]).ok()?;
    Ok(out.stdout.trim().to_owned())
}

pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(repo)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .run()
        .map(|o| o.code == 0)
        .unwrap_or(false)
}

/// worktree を作る。既にあれば再利用。ブランチが既にあればそれを checkout、無ければ base から切る。
pub fn worktree_add(repo: &Path, dir: &Path, branch: &str, base: &str) -> Result<bool> {
    if dir.is_dir() {
        return Ok(false);
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dir_s = dir.to_string_lossy().into_owned();
    if branch_exists(repo, branch) {
        git(repo).args(["worktree", "add", &dir_s, branch]).ok()?;
    } else {
        git(repo)
            .args(["worktree", "add", "-b", branch, &dir_s, base])
            .ok()?;
    }
    Ok(true)
}
