pub(crate) mod catalog_cache;
pub(crate) mod catalog_loader;
pub(crate) mod catalog_sync;
pub(crate) mod skill_resolver;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use self::catalog_cache::CatalogCache;
use super::fs::SkillMirror;
use super::skill::Skill;
use super::skill::SkillScope;
use crate::base::Result;
use crate::storage::AgentDatabases;

#[async_trait]
pub trait SkillCatalog: Send + Sync + 'static {
    fn for_agent(&self, agent_id: &str, user_id: &str) -> Vec<Skill>;
    fn get(&self, name: &str) -> Option<Skill>;
    async fn reload(&self) -> Result<()>;
    fn insert(&self, skill: &Skill);
    fn evict(&self, name: &str);
    fn resolve(&self, tool_name: &str) -> Option<Skill>;
    fn script_path(&self, tool_name: &str) -> Option<String>;
    fn host_script_path(&self, tool_name: &str) -> Option<PathBuf>;
    fn read_skill(&self, path: &str) -> Option<String>;
}

pub fn scope_priority(scope: &SkillScope) -> u8 {
    match scope {
        SkillScope::Agent => 2,
        SkillScope::User => 1,
        SkillScope::Global => 0,
    }
}

pub fn should_replace(existing: &Skill, candidate: &Skill) -> bool {
    let ep = scope_priority(&existing.scope);
    let cp = scope_priority(&candidate.scope);
    if cp != ep {
        return cp > ep;
    }
    let existing_key = (
        existing.agent_id.as_deref().unwrap_or(""),
        existing.user_id.as_deref().unwrap_or(""),
        existing.name.as_str(),
    );
    let candidate_key = (
        candidate.agent_id.as_deref().unwrap_or(""),
        candidate.user_id.as_deref().unwrap_or(""),
        candidate.name.as_str(),
    );
    candidate_key > existing_key
}

pub struct SkillCatalogImpl {
    databases: Arc<AgentDatabases>,
    mirror: SkillMirror,
    cache: CatalogCache,
}

impl SkillCatalogImpl {
    pub fn new(databases: Arc<AgentDatabases>, local_dir: PathBuf) -> Self {
        Self {
            databases,
            mirror: SkillMirror::new(local_dir),
            cache: CatalogCache::new(),
        }
    }

    pub fn loaded_skills(&self) -> Vec<super::fs::LoadedSkill> {
        self.cache.all_loaded()
    }

    pub async fn load(&self) -> Result<()> {
        catalog_loader::load_builtins(&self.mirror, &self.cache);
        self.sync().await
    }

    pub async fn sync(&self) -> Result<()> {
        catalog_sync::sync(&self.databases, &self.mirror, &self.cache).await
    }

    pub fn log_loaded_skills(&self) {
        catalog_loader::log_loaded_skills(&self.cache);
    }
}

#[async_trait]
impl SkillCatalog for SkillCatalogImpl {
    fn for_agent(&self, agent_id: &str, user_id: &str) -> Vec<Skill> {
        let result = self.cache.for_agent(agent_id, user_id);
        let total_cached = self.cache.all_loaded().len();
        tracing::info!(
            agent_id,
            user_id,
            total_cached,
            visible = result.len(),
            "catalog.for_agent: filtered skills by visibility"
        );
        for s in &result {
            tracing::info!(
                name = %s.name, scope = %s.scope, source = %s.source,
                executable = s.executable, content_size = s.content.len(),
                description = %s.description,
                "catalog.for_agent: skill visible"
            );
        }
        result
    }

    fn get(&self, name: &str) -> Option<Skill> {
        let result = self.cache.get(name);
        match &result {
            Some(s) => tracing::info!(
                name = %s.name, scope = %s.scope, source = %s.source,
                content_size = s.content.len(), "catalog.get: skill found"
            ),
            None => tracing::info!(name, "catalog.get: skill not found"),
        }
        result
    }

    async fn reload(&self) -> Result<()> {
        tracing::info!("catalog.reload: starting sync");
        self.sync().await
    }

    fn insert(&self, skill: &Skill) {
        tracing::info!(name = %skill.name, scope = %skill.scope, "catalog.insert: writing skill");
        if let Some(loaded) = self.mirror.write_skill(skill) {
            self.cache.insert(skill.name.clone(), loaded);
            tracing::info!(name = %skill.name, "catalog.insert: skill cached");
        }
    }

    fn evict(&self, name: &str) {
        tracing::info!(name, "catalog.evict: removing skill");
        self.mirror.remove_skill(name);
        self.cache.remove(name);
    }

    fn resolve(&self, tool_name: &str) -> Option<Skill> {
        skill_resolver::resolve(&self.cache, tool_name)
    }

    fn script_path(&self, tool_name: &str) -> Option<String> {
        skill_resolver::script_path(&self.cache, &self.mirror, tool_name)
    }

    fn host_script_path(&self, tool_name: &str) -> Option<PathBuf> {
        skill_resolver::host_script_path(&self.cache, tool_name)
    }

    fn read_skill(&self, path: &str) -> Option<String> {
        skill_resolver::read_skill(&self.cache, path)
    }
}

pub use catalog_sync::spawn_sync_task;
