use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use serde_json::{json, Value};

use super::Ctx;
use crate::config;

const SKILL: &str = include_str!("../../assets/SKILL.md");
const HOOK_TIMEOUT: u64 = 10;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// `todo` シンボリックリンクを張らない
    #[arg(long)]
    pub no_link: bool,
    /// Claude Code の設定ディレクトリ。config.toml に記録する (既定: CLAUDE_CONFIG_DIR > config.toml > ~/.claude)
    #[arg(long, value_name = "PATH")]
    pub claude_config_dir: Option<PathBuf>,
}

pub fn run(ctx: &Ctx, a: Args) -> Result<()> {
    ctx.store.ensure()?;
    // 明示された時 (フラグ / env / config) だけ config.toml に残す。既定の ~/.claude は書かない
    let explicit = a
        .claude_config_dir
        .as_deref()
        .map(config::expand_tilde)
        .or_else(|| ctx.config.claude_config_dir());
    let cfg = match &explicit {
        Some(d) => d.clone(),
        None => dirs::home_dir()
            .context("home directory not found")?
            .join(".claude"),
    };
    let exe = std::env::current_exe()?.canonicalize()?;
    let exe_s = exe.to_string_lossy().into_owned();

    // config.toml
    let config_path = config::Config::path(&ctx.store);
    let config_changed = if config_path.exists() {
        match &explicit {
            Some(d) => config::set_claude_config_dir(&config_path, d)?,
            None => false,
        }
    } else {
        std::fs::write(&config_path, config::template(explicit.as_deref()))?;
        true
    };

    // hooks
    let settings = cfg.join("settings.json");
    let mut v: Value = match std::fs::read(&settings) {
        Ok(b) => {
            serde_json::from_slice(&b).with_context(|| format!("parse {}", settings.display()))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e).with_context(|| format!("read {}", settings.display())),
    };
    let mut changed = ensure_hook(&mut v, "Stop", &format!("{exe_s} hook stop"));
    changed |= ensure_hook(&mut v, "SessionEnd", &format!("{exe_s} hook session-end"));
    if changed {
        std::fs::create_dir_all(&cfg)?;
        let mut s = serde_json::to_string_pretty(&v)?;
        s.push('\n');
        std::fs::write(&settings, s)?;
    }

    // skill
    let skill_dir = cfg.join("skills").join("kotoshiro");
    std::fs::create_dir_all(&skill_dir)?;
    let skill = skill_dir.join("SKILL.md");
    let skill_changed = std::fs::read_to_string(&skill).ok().as_deref() != Some(SKILL);
    if skill_changed {
        std::fs::write(&skill, SKILL)?;
    }

    // todo link
    let link = if a.no_link { None } else { make_link(&exe)? };

    if ctx.json {
        ctx.print_json(&json!({
            "store": ctx.store.root(),
            "config": config_path,
            "config_changed": config_changed,
            "claude_config_dir": cfg,
            "settings": settings,
            "hooks_changed": changed,
            "skill": skill,
            "skill_changed": skill_changed,
            "link": link,
        }))
    } else {
        println!("store:    {}", ctx.store.root().display());
        println!(
            "config:   {} ({})",
            config_path.display(),
            if config_changed { "updated" } else { "ok" }
        );
        println!(
            "hooks:    {} ({})",
            settings.display(),
            if changed { "updated" } else { "ok" }
        );
        println!(
            "skill:    {} ({})",
            skill.display(),
            if skill_changed { "updated" } else { "ok" }
        );
        if let Some(l) = link {
            println!("link:     {}", l.display());
        }
        Ok(())
    }
}

/// `hooks.<event>` に command を 1 つだけ持たせる。古いパスの kotoshiro エントリは置き換える。
/// 他ツールの hook は触らない。変更があれば true。
pub fn ensure_hook(root: &mut Value, event: &str, command: &str) -> bool {
    if !root.is_object() {
        *root = json!({});
    }
    let hooks = root
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let arr = hooks
        .as_object_mut()
        .unwrap()
        .entry(event)
        .or_insert_with(|| json!([]));
    if !arr.is_array() {
        *arr = json!([]);
    }
    let groups = arr.as_array_mut().unwrap();

    let suffix = format!(" hook {}", event_suffix(event));
    let is_ours = |h: &Value| {
        h.get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| c.ends_with(&suffix) && c.contains("kotoshiro"))
    };
    let exact = groups.iter().any(|g| {
        g.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
            hs.iter()
                .any(|h| h.get("command").and_then(Value::as_str) == Some(command))
        })
    });
    if exact {
        return false;
    }
    for g in groups.iter_mut() {
        if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) {
            hs.retain(|h| !is_ours(h));
        }
    }
    groups.retain(|g| {
        g.get("hooks")
            .and_then(Value::as_array)
            .is_none_or(|hs| !hs.is_empty())
    });
    groups.push(json!({
        "hooks": [{ "type": "command", "command": command, "timeout": HOOK_TIMEOUT }]
    }));
    true
}

fn event_suffix(event: &str) -> &'static str {
    match event {
        "Stop" => "stop",
        "SessionEnd" => "session-end",
        _ => "",
    }
}

fn make_link(exe: &Path) -> Result<Option<PathBuf>> {
    let dir = exe.parent().context("exe has no parent")?;
    let link = dir.join("todo");
    match std::fs::symlink_metadata(&link) {
        Ok(md) if md.file_type().is_symlink() => {
            let target = std::fs::read_link(&link)?;
            let resolved = if target.is_absolute() {
                target
            } else {
                dir.join(target)
            };
            if resolved.canonicalize().ok().as_deref() == Some(exe) {
                return Ok(Some(link));
            }
            eprintln!(
                "warning: {} points elsewhere ({}); not touching it",
                link.display(),
                resolved.display()
            );
            Ok(None)
        }
        Ok(_) => {
            eprintln!(
                "warning: {} exists and is not a symlink; not touching it",
                link.display()
            );
            Ok(None)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(unix)]
            std::os::unix::fs::symlink(exe, &link)
                .with_context(|| format!("symlink {}", link.display()))?;
            Ok(Some(link))
        }
        Err(e) => Err(e).with_context(|| format!("stat {}", link.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_merge_is_idempotent_and_preserves_others() {
        let mut v = json!({
            "permissions": {"allow": ["Bash"]},
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "other-tool stop"}]}],
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "x"}]}]
            }
        });
        assert!(ensure_hook(&mut v, "Stop", "/a/kotoshiro hook stop"));
        assert!(ensure_hook(
            &mut v,
            "SessionEnd",
            "/a/kotoshiro hook session-end"
        ));
        assert!(!ensure_hook(&mut v, "Stop", "/a/kotoshiro hook stop"));
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "other-tool stop");
        assert_eq!(stop[1]["hooks"][0]["command"], "/a/kotoshiro hook stop");
        assert_eq!(v["permissions"]["allow"][0], "Bash");
        assert!(v["hooks"]["PreToolUse"].is_array());

        // パスが変わったら置き換わる
        assert!(ensure_hook(&mut v, "Stop", "/b/kotoshiro hook stop"));
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[1]["hooks"][0]["command"], "/b/kotoshiro hook stop");
    }

    #[test]
    fn hook_merge_from_empty() {
        let mut v = json!({});
        assert!(ensure_hook(&mut v, "Stop", "k hook stop"));
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["timeout"], HOOK_TIMEOUT);
    }
}
