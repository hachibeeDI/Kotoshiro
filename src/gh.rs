//! `gh` への依存はここに集める。kotoshiro が `--json <fields>` で形を指定するので厳格に読む。

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::proc::Cmd;

const TIMEOUT: Duration = Duration::from_secs(60);

fn gh() -> Cmd {
    Cmd::new("gh").timeout(TIMEOUT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoView {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

/// リポジトリの `owner/name`。
pub fn name_with_owner(repo: &Path) -> Result<String> {
    let out = gh()
        .cwd(repo)
        .args(["repo", "view", "--json", "nameWithOwner"])
        .ok()?;
    let v: RepoView = serde_json::from_str(out.stdout.trim()).context("parse gh repo view")?;
    Ok(v.name_with_owner)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pr {
    pub number: u64,
    /// OPEN | CLOSED | MERGED
    pub state: String,
    pub url: String,
    pub title: String,
}

/// ブランチを head に持つ PR (状態問わず)。
pub fn prs_for_branch(nwo: &str, branch: &str) -> Result<Vec<Pr>> {
    let out = gh()
        .args(["pr", "list", "--repo", nwo, "--head", branch])
        .args(["--state", "all", "--json", "number,state,url,title"])
        .ok()?;
    serde_json::from_str(out.stdout.trim()).context("parse gh pr list")
}

pub fn issue_comment(nwo: &str, number: u64, body: &str) -> Result<()> {
    gh().args(["issue", "comment", &number.to_string(), "--repo", nwo])
        .args(["--body-file", "-"])
        .stdin(body)
        .ok()?;
    Ok(())
}

/// 作成した issue の URL。
pub fn issue_create(nwo: &str, title: &str, body: &str) -> Result<String> {
    let out = gh()
        .args(["issue", "create", "--repo", nwo, "--title", title])
        .args(["--body-file", "-"])
        .stdin(body)
        .ok()?;
    Ok(out.stdout.trim().to_owned())
}
