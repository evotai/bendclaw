//! Sync skills from agent databases to per-agent .remote/ directories.
//!
//! No in-memory cache — the local filesystem mirror IS the cache.

use std::collections::HashSet;
use std::sync::Arc;

use super::paths;
use super::repository::DatabendSkillRepository;
use super::repository::SkillRepository;
use super::writer;
use crate::base::Result;
use crate::kernel::skills::skill::Skill;
use crate::observability::log::slog;
use crate::storage::AgentDatabases;

pub async fn sync(databases: &Arc<AgentDatabases>, workspace_root: &std::path::Path) -> Result<()> {
    let db_names = match databases.list_databases().await {
        Ok(dbs) => dbs,
        Err(e) => {
            slog!(warn, "skill_sync", "db_list_failed", error = %e,);
            return Ok(());
        }
    };

    let all_skills = fetch_all(databases, &db_names).await;

    slog!(
        debug,
        "skill_sync",
        "fetched",
        databases = db_names.len(),
        skills = all_skills.len(),
    );

    // Collect live keys so we can detect stale dirs
    let mut live_keys: HashSet<(String, String)> = HashSet::new();

    // Write skills to local mirror (skip unchanged)
    for skill in &all_skills {
        let agent_id = skill.agent_id.as_deref().unwrap_or("_global");
        live_keys.insert((agent_id.to_string(), skill.name.clone()));

        // Skip if the on-disk version matches
        let db_checksum = skill.compute_sha256();
        if let Some(disk_checksum) =
            writer::read_disk_checksum(workspace_root, agent_id, &skill.name)
        {
            if disk_checksum == db_checksum {
                continue;
            }
        }

        let remote_dir = paths::remote_dir(workspace_root, agent_id);
        let _ = std::fs::create_dir_all(&remote_dir);
        writer::write_skill(workspace_root, agent_id, skill);
    }

    // Remove stale skill dirs that are no longer in DB
    evict_stale(workspace_root, &live_keys);

    Ok(())
}

/// Remove `.remote/` skill directories that no longer exist in any agent DB.
fn evict_stale(workspace_root: &std::path::Path, live_keys: &HashSet<(String, String)>) {
    let agents_dir = workspace_root.join("agents");
    let entries = match std::fs::read_dir(&agents_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let agent_dir = entry.path();
        if !agent_dir.is_dir() {
            continue;
        }
        let agent_id = match agent_dir.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let remote_dir = agent_dir.join("skills").join(".remote");
        let skill_entries = match std::fs::read_dir(&remote_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for skill_entry in skill_entries.flatten() {
            let skill_dir = skill_entry.path();
            if !skill_dir.is_dir() {
                continue;
            }
            let skill_name = match skill_dir.file_name().and_then(|n| n.to_str()) {
                Some(n) if !n.starts_with('.') => n.to_string(),
                _ => continue,
            };
            if !live_keys.contains(&(agent_id.clone(), skill_name.clone())) {
                let _ = std::fs::remove_dir_all(&skill_dir);
            }
        }
    }
}

async fn fetch_all(databases: &Arc<AgentDatabases>, db_names: &[String]) -> Vec<Skill> {
    let mut all_skills: Vec<Skill> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for db_name in db_names {
        let pool = match databases.root_pool().with_database(db_name) {
            Ok(p) => p,
            Err(e) => {
                slog!(warn, "skill_sync", "db_skipped", db = %db_name, error = %e,);
                continue;
            }
        };
        let store = DatabendSkillRepository::new(pool);
        match store.list().await {
            Ok(skills) => {
                for skill in skills {
                    let key = (
                        skill.agent_id.clone().unwrap_or_default(),
                        skill.name.clone(),
                    );
                    if seen.contains(&key) {
                        continue;
                    }
                    match store.get(&skill.name).await {
                        Ok(Some(full)) => {
                            seen.insert(key);
                            all_skills.push(full);
                        }
                        Ok(None) => {
                            seen.insert(key);
                            all_skills.push(skill);
                        }
                        Err(e) => {
                            slog!(warn, "skill_sync", "skill_fetch_failed", skill = %skill.name, error = %e,);
                            seen.insert(key);
                            all_skills.push(skill);
                        }
                    }
                }
            }
            Err(e) => {
                slog!(warn, "skill_sync", "skill_list_failed", db = %db_name, error = %e,);
            }
        }
    }

    all_skills
}

pub fn spawn_sync_task(
    store: Arc<crate::kernel::skills::store::SkillStore>,
    interval_secs: u64,
    cancel: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    crate::base::spawn_named("skill_sync_loop", async move {
        let base_interval = std::time::Duration::from_secs(interval_secs);
        let mut consecutive_errors: u64 = 0;
        // First refresh fires immediately so the server starts without
        // blocking on skill sync, yet skills become available quickly.
        let mut next_sleep = std::time::Duration::ZERO;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => { break; }
                _ = tokio::time::sleep(next_sleep) => {
                    if let Err(e) = store.refresh().await {
                        consecutive_errors += 1;
                        if consecutive_errors == 1 || consecutive_errors.is_multiple_of(20) {
                            slog!(error, "skill_sync", "failed", error = %e, consecutive_errors,);
                        }
                        // Exponential backoff: 60s, 120s, 240s, capped at 300s
                        let secs = (60u64 << (consecutive_errors - 1).min(3)).min(300);
                        next_sleep = std::time::Duration::from_secs(secs);
                    } else {
                        consecutive_errors = 0;
                        next_sleep = base_interval;
                    }
                }
            }
        }
    })
}
