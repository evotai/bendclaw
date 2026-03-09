use std::collections::HashMap;

use parking_lot::RwLock;

use crate::kernel::skills::fs::LoadedSkill;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillSource;

pub(super) struct CatalogCache {
    cache: RwLock<HashMap<String, LoadedSkill>>,
}

impl CatalogCache {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn get(&self, name: &str) -> Option<Skill> {
        self.cache.read().get(name).map(|s| s.skill.clone())
    }

    pub fn resolve(&self, tool_name: &str) -> Option<LoadedSkill> {
        let cache = self.cache.read();
        if let Some(loaded) = cache.get(tool_name) {
            return Some(loaded.clone());
        }
        tool_name
            .find('/')
            .and_then(|idx| cache.get(&tool_name[idx + 1..]).cloned())
    }

    pub fn split_doc_path(&self, path: &str) -> Option<(LoadedSkill, String)> {
        if path.contains("..") {
            return None;
        }
        let cache = self.cache.read();
        if let Some(loaded) = cache.get(path) {
            return Some((loaded.clone(), String::new()));
        }
        for (idx, _) in path.match_indices('/').rev() {
            if let Some(loaded) = cache.get(&path[..idx]) {
                return Some((loaded.clone(), path[idx + 1..].to_string()));
            }
        }
        None
    }

    pub fn insert(&self, name: String, loaded: LoadedSkill) {
        self.cache.write().insert(name, loaded);
    }

    pub fn remove(&self, name: &str) {
        self.cache.write().remove(name);
    }

    pub fn all_loaded(&self) -> Vec<LoadedSkill> {
        self.cache.read().values().cloned().collect()
    }

    pub fn for_agent(&self, agent_id: &str, user_id: &str) -> Vec<Skill> {
        self.cache
            .read()
            .values()
            .filter(|s| s.skill.is_visible_to(agent_id, user_id))
            .map(|s| s.skill.clone())
            .collect()
    }

    pub fn stale_remote_keys(&self, live_keys: &HashMap<String, Skill>) -> Vec<String> {
        self.cache
            .read()
            .iter()
            .filter(|(_, loaded)| loaded.skill.source != SkillSource::Local)
            .map(|(name, _)| name.clone())
            .filter(|name| !live_keys.contains_key(name))
            .collect()
    }

    pub fn has_matching_checksum(&self, name: &str, sha256: &str) -> bool {
        self.cache
            .read()
            .get(name)
            .map(|existing| existing.skill.compute_sha256() == sha256)
            .unwrap_or(false)
    }
}
