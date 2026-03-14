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
use crate::storage::AgentDatabases;

pub async fn sync(databases: &Arc<AgentDatabases>, workspace_root: &std::path::Path) -> Result<()> {
    let db_names = match databases.list_databases().await {
        Ok(dbs) => dbs,
        Err(e) => {
            tracing::warn!(error = %e, "failed to list agent databases for skill sync");
            return Ok(());
        }
    };

    let all_skills = fetch_all(databases, &db_names).await;

    tracing::debug!(
        databases = db_names.len(),
        skills = all_skills.len(),
        "skill sync from agent databases"
    );

    // Collect live keys so we can detect stale dirs
    let mut live_keys: HashSet<(String, String)> = HashSet::new();

    // Write skills to local mirror (skip unchanged)
    let mut written = 0usize;
    let mut skipped = 0usize;
    for skill in &all_skills {
        let agent_id = skill.agent_id.as_deref().unwrap_or("_global");
        live_keys.insert((agent_id.to_string(), skill.name.clone()));

        // Skip if the on-disk version matches
        let db_checksum = skill.compute_sha256();
        if let Some(disk_checksum) =
            writer::read_disk_checksum(workspace_root, agent_id, &skill.name)
        {
            if disk_checksum == db_checksum {
                skipped += 1;
                continue;
            }
        }

        let remote_dir = paths::remote_dir(workspace_root, agent_id);
        let _ = std::fs::create_dir_all(&remote_dir);
        writer::write_skill(workspace_root, agent_id, skill);
        written += 1;
    }

    if written > 0 || skipped > 0 {
        tracing::info!(written, skipped, "skill sync write summary");
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
    let mut removed = 0usize;
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
                removed += 1;
            }
        }
    }
    if removed > 0 {
        tracing::info!(count = removed, "evicted stale remote skill directories");
    }
}

async fn fetch_all(databases: &Arc<AgentDatabases>, db_names: &[String]) -> Vec<Skill> {
    let mut all_skills: Vec<Skill> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for db_name in db_names {
        let pool = match databases.root_pool().with_database(db_name) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(db = %db_name, error = %e, "skip database for skill sync");
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
                            tracing::warn!(
                                skill = %skill.name,
                                error = %e,
                                "failed to fetch skill files, using metadata only"
                            );
                            seen.insert(key);
                            all_skills.push(skill);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(db = %db_name, error = %e, "failed to list skills from database");
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
    tokio::spawn(async move {
        let base_interval = std::time::Duration::from_secs(interval_secs);
        let mut interval = tokio::time::interval(base_interval);
        interval.tick().await;
        let mut consecutive_errors: u64 = 0;
        loop {
            let sleep_dur = if consecutive_errors > 0 {
                // Exponential backoff: 60s, 120s, 240s, capped at 300s
                let secs = (60u64 << (consecutive_errors - 1).min(3)).min(300);
                std::time::Duration::from_secs(secs)
            } else {
                base_interval
            };

            tokio::select! {
                _ = cancel.cancelled() => { tracing::info!("skill sync task cancelled"); break; }
                _ = tokio::time::sleep(sleep_dur) => {
                    if let Err(e) = store.refresh().await {
                        consecutive_errors += 1;
                        if consecutive_errors == 1 || consecutive_errors.is_multiple_of(20) {
                            tracing::error!(
                                error = %e,
                                consecutive_errors,
                                "skill sync failed"
                            );
                        }
                    } else {
                        if consecutive_errors > 0 {
                            tracing::info!(consecutive_errors, "skill sync recovered");
                        }
                        consecutive_errors = 0;
                    }
                }
            }
        }
    })
}
