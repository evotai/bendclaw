//! Hub sync: git clone/pull from the official skills repository.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::time::SystemTime;

use super::paths;
use crate::kernel::skills::diagnostics;

const DEFAULT_REPO_URL: &str = "https://github.com/EvotAI/skills";

/// Ensure the hub directory exists and is up-to-date. Returns the hub dir path if available.
pub fn ensure(workspace_root: &Path, repo_url: &str, interval_secs: u64) -> Option<PathBuf> {
    if is_disabled() {
        return None;
    }
    if let Some(d) = env_override_dir() {
        return d.exists().then_some(d);
    }
    if interval_secs == 0 {
        return None;
    }

    let hub_dir = paths::hub_dir(workspace_root);
    let url = if repo_url.is_empty() {
        DEFAULT_REPO_URL
    } else {
        repo_url
    };

    if !hub_dir.exists() {
        git_clone(url, &hub_dir)?;
        let _ = mark_synced(&hub_dir);
    } else if should_sync(&hub_dir, interval_secs) && git_pull(&hub_dir) {
        let _ = mark_synced(&hub_dir);
    }
    Some(hub_dir)
}

pub fn last_sync_time(workspace_root: &Path) -> Option<SystemTime> {
    let hub_dir = paths::hub_dir(workspace_root);
    std::fs::metadata(hub_dir.join(paths::SYNC_MARKER))
        .and_then(|m| m.modified())
        .ok()
}

fn is_disabled() -> bool {
    std::env::var("BENDCLAW_HUB_DISABLED")
        .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn env_override_dir() -> Option<PathBuf> {
    std::env::var("BENDCLAW_HUB_SKILLS_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
}

fn git_clone(url: &str, target: &Path) -> Option<()> {
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let out = Command::new("git")
        .args(["clone", "--depth", "1", url])
        .arg(target)
        .output()
        .ok()?;
    if out.status.success() {
        Some(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        diagnostics::log_skill_hub_clone_failed(&stderr);
        None
    }
}

fn git_pull(repo_dir: &Path) -> bool {
    if !repo_dir.join(".git").exists() {
        return true;
    }
    Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["pull", "--ff-only"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn should_sync(hub_dir: &Path, interval_secs: u64) -> bool {
    std::fs::metadata(hub_dir.join(paths::SYNC_MARKER))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|age| age >= Duration::from_secs(interval_secs))
        .unwrap_or(true)
}

pub fn mark_synced(hub_dir: &Path) -> std::io::Result<()> {
    std::fs::write(hub_dir.join(paths::SYNC_MARKER), b"synced")
}
