use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::skills::repository::SkillRepository;
use bendclaw::kernel::skills::repository::SkillRepositoryFactory;
use bendclaw::kernel::skills::skill::Skill;
use parking_lot::Mutex;

/// Skill catalog that has no skills.
pub struct NoopSkillCatalog;

#[async_trait]
impl SkillCatalog for NoopSkillCatalog {
    fn for_agent(&self, _agent_id: &str, _user_id: &str) -> Vec<Skill> {
        vec![]
    }
    fn get(&self, _name: &str) -> Option<Skill> {
        None
    }
    async fn reload(&self) -> bendclaw::base::Result<()> {
        Ok(())
    }
    fn insert(&self, _skill: &Skill) {}
    fn evict(&self, _name: &str) {}
    fn resolve(&self, _tool_name: &str) -> Option<Skill> {
        None
    }
    fn script_path(&self, _tool_name: &str) -> Option<String> {
        None
    }
    fn host_script_path(&self, _tool_name: &str) -> Option<PathBuf> {
        None
    }
    fn read_skill(&self, _path: &str) -> Option<String> {
        None
    }
}

/// In-memory skill catalog for tests.
pub struct MockSkillCatalog {
    skills: Mutex<HashMap<String, Skill>>,
}

impl MockSkillCatalog {
    pub fn new() -> Self {
        Self {
            skills: Mutex::new(HashMap::new()),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.skills.lock().contains_key(name)
    }
}

#[async_trait]
impl SkillCatalog for MockSkillCatalog {
    fn for_agent(&self, _agent_id: &str, _user_id: &str) -> Vec<Skill> {
        self.skills.lock().values().cloned().collect()
    }

    fn get(&self, name: &str) -> Option<Skill> {
        self.skills.lock().get(name).cloned()
    }

    async fn reload(&self) -> bendclaw::base::Result<()> {
        Ok(())
    }

    fn insert(&self, skill: &Skill) {
        self.skills.lock().insert(skill.name.clone(), skill.clone());
    }

    fn evict(&self, name: &str) {
        self.skills.lock().remove(name);
    }

    fn resolve(&self, tool_name: &str) -> Option<Skill> {
        self.skills.lock().get(tool_name).cloned()
    }

    fn script_path(&self, tool_name: &str) -> Option<String> {
        self.resolve(tool_name).and_then(|skill| {
            skill
                .files
                .iter()
                .find(|f| f.path.starts_with("scripts/"))
                .map(|f| f.path.clone())
        })
    }

    fn host_script_path(&self, _tool_name: &str) -> Option<PathBuf> {
        None
    }

    fn read_skill(&self, path: &str) -> Option<String> {
        self.skills.lock().get(path).map(|s| s.content.clone())
    }
}

/// Skill store that does nothing (for tests that don't need persistence).
pub struct NoopSkillStore;

#[async_trait]
impl SkillRepository for NoopSkillStore {
    async fn list(&self) -> bendclaw::base::Result<Vec<Skill>> {
        Ok(vec![])
    }
    async fn get(&self, _name: &str) -> bendclaw::base::Result<Option<Skill>> {
        Ok(None)
    }
    async fn save(&self, _skill: &Skill) -> bendclaw::base::Result<()> {
        Ok(())
    }
    async fn remove(
        &self,
        _name: &str,
        _agent_id: Option<&str>,
        _user_id: Option<&str>,
    ) -> bendclaw::base::Result<()> {
        Ok(())
    }
    async fn checksums(&self) -> bendclaw::base::Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }
}

/// In-memory [`SkillRepository`] for unit-testing tools that create/remove skills.
pub struct MockSkillStore {
    skills: Mutex<HashMap<String, Skill>>,
}

impl MockSkillStore {
    pub fn new() -> Self {
        Self {
            skills: Mutex::new(HashMap::new()),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.skills.lock().contains_key(name)
    }

    pub fn get_skill(&self, name: &str) -> Option<Skill> {
        self.skills.lock().get(name).cloned()
    }
}

#[async_trait]
impl SkillRepository for MockSkillStore {
    async fn list(&self) -> bendclaw::base::Result<Vec<Skill>> {
        Ok(self.skills.lock().values().cloned().collect())
    }

    async fn get(&self, name: &str) -> bendclaw::base::Result<Option<Skill>> {
        Ok(self.skills.lock().get(name).cloned())
    }

    async fn save(&self, skill: &Skill) -> bendclaw::base::Result<()> {
        self.skills.lock().insert(skill.name.clone(), skill.clone());
        Ok(())
    }

    async fn remove(
        &self,
        name: &str,
        _agent_id: Option<&str>,
        _user_id: Option<&str>,
    ) -> bendclaw::base::Result<()> {
        self.skills.lock().remove(name);
        Ok(())
    }

    async fn checksums(&self) -> bendclaw::base::Result<HashMap<String, String>> {
        let map = self
            .skills
            .lock()
            .iter()
            .map(|(k, v)| (k.clone(), v.compute_sha256()))
            .collect();
        Ok(map)
    }
}

/// Factory that always returns a fresh [`MockSkillStore`].
pub struct MockSkillStoreFactory;

impl SkillRepositoryFactory for MockSkillStoreFactory {
    fn for_agent(
        &self,
        _agent_id: &str,
    ) -> bendclaw::base::Result<std::sync::Arc<dyn SkillRepository>> {
        Ok(std::sync::Arc::new(MockSkillStore::new()))
    }
}
