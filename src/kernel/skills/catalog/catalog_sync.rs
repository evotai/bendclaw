use std::collections::HashMap;
use std::sync::Arc;

use super::should_replace;
use crate::base::Result;
use crate::kernel::skills::catalog::catalog_cache::CatalogCache;
use crate::kernel::skills::fs::SkillMirror;
use crate::kernel::skills::repository::DatabendSkillRepository;
use crate::kernel::skills::repository::SkillRepository;
use crate::kernel::skills::skill::Skill;
use crate::storage::AgentDatabases;

pub(super) async fn sync(
    databases: &Arc<AgentDatabases>,
    mirror: &SkillMirror,
    cache: &CatalogCache,
) -> Result<()> {
    let db_names = match databases.list_databases().await {
        Ok(dbs) => dbs,
        Err(e) => {
            tracing::warn!(error = %e, "failed to list agent databases for skill sync");
            return Ok(());
        }
    };

    let all_skills = fetch_all(databases, &db_names).await;

    tracing::info!(
        databases = db_names.len(),
        skills = all_skills.len(),
        "skill sync from agent databases"
    );

    // Evict stale remote skills
    let stale = cache.stale_remote_keys(&all_skills);
    for key in &stale {
        cache.remove(key);
        mirror.remove_remote_skill(key);
    }
    if !stale.is_empty() {
        tracing::info!(count = stale.len(), "evicted stale remote skills");
    }

    // Write new/updated remote skills
    for (name, skill) in all_skills {
        if cache.has_matching_checksum(&name, &skill.compute_sha256()) {
            continue;
        }
        if let Some(loaded) = mirror.write_remote_skill(&name, &skill) {
            cache.insert(name, loaded);
        }
    }

    Ok(())
}

async fn fetch_all(databases: &Arc<AgentDatabases>, db_names: &[String]) -> HashMap<String, Skill> {
    let mut all_skills: HashMap<String, Skill> = HashMap::new();

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
                    let replace = all_skills
                        .get(&skill.name)
                        .map(|existing| should_replace(existing, &skill))
                        .unwrap_or(true);
                    if replace {
                        match store.get(&skill.name).await {
                            Ok(Some(full)) => {
                                all_skills.insert(full.name.clone(), full);
                            }
                            Ok(None) => {
                                all_skills.insert(skill.name.clone(), skill);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    skill = %skill.name,
                                    error = %e,
                                    "failed to fetch skill files, using metadata only"
                                );
                                all_skills.insert(skill.name.clone(), skill);
                            }
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
    store: Arc<super::SkillCatalogImpl>,
    interval_secs: u64,
    cancel: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => { tracing::info!("skill sync task cancelled"); break; }
                _ = interval.tick() => { if let Err(e) = store.sync().await { tracing::error!(error = %e, "skill sync failed"); } }
            }
        }
    })
}
