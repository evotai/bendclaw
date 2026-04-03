use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::execution::skills::UsageSink;
use bendclaw::skills::definition::skill::Skill;
use bendclaw::skills::store::SharedSkillStore;
use bendclaw::skills::sync::SkillWriter;
use bendclaw::subscriptions::store::Subscription;
use bendclaw::subscriptions::store::SubscriptionStore;
use parking_lot::Mutex;

/// Skill store that does nothing (for tests that don't need persistence).
pub struct NoopSkillStore;

#[async_trait]
impl SharedSkillStore for NoopSkillStore {
    async fn list(&self, _user_id: &str) -> bendclaw::types::Result<Vec<Skill>> {
        Ok(vec![])
    }
    async fn get(&self, _user_id: &str, _name: &str) -> bendclaw::types::Result<Option<Skill>> {
        Ok(None)
    }
    async fn save(&self, _user_id: &str, _skill: &Skill) -> bendclaw::types::Result<()> {
        Ok(())
    }
    async fn remove(&self, _user_id: &str, _name: &str) -> bendclaw::types::Result<()> {
        Ok(())
    }
    async fn checksums(&self, _user_id: &str) -> bendclaw::types::Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }
    async fn touch_last_used(
        &self,
        _id: &bendclaw::skills::definition::skill::SkillId,
        _agent_id: &str,
    ) -> bendclaw::types::Result<()> {
        Ok(())
    }
    async fn list_shared(&self, _user_id: &str) -> bendclaw::types::Result<Vec<Skill>> {
        Ok(vec![])
    }
}

/// In-memory skill store for unit-testing tools that create/remove skills.
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
impl SharedSkillStore for MockSkillStore {
    async fn list(&self, _user_id: &str) -> bendclaw::types::Result<Vec<Skill>> {
        Ok(self.skills.lock().values().cloned().collect())
    }

    async fn get(&self, _user_id: &str, name: &str) -> bendclaw::types::Result<Option<Skill>> {
        Ok(self.skills.lock().get(name).cloned())
    }

    async fn save(&self, _user_id: &str, skill: &Skill) -> bendclaw::types::Result<()> {
        self.skills.lock().insert(skill.name.clone(), skill.clone());
        Ok(())
    }

    async fn remove(&self, _user_id: &str, name: &str) -> bendclaw::types::Result<()> {
        self.skills.lock().remove(name);
        Ok(())
    }

    async fn checksums(&self, _user_id: &str) -> bendclaw::types::Result<HashMap<String, String>> {
        let map = self
            .skills
            .lock()
            .iter()
            .map(|(k, v)| (k.clone(), v.compute_sha256()))
            .collect();
        Ok(map)
    }

    async fn touch_last_used(
        &self,
        _id: &bendclaw::skills::definition::skill::SkillId,
        _agent_id: &str,
    ) -> bendclaw::types::Result<()> {
        Ok(())
    }

    async fn list_shared(&self, _user_id: &str) -> bendclaw::types::Result<Vec<Skill>> {
        Ok(vec![])
    }
}

/// Subscription store that does nothing (for tests that don't need subscription persistence).
pub struct NoopSubscriptionStore;

#[async_trait]
impl SubscriptionStore for NoopSubscriptionStore {
    async fn subscribe(
        &self,
        _user_id: &str,
        _resource_type: &str,
        _resource_key: &str,
        _owner_id: &str,
    ) -> bendclaw::types::Result<()> {
        Ok(())
    }

    async fn unsubscribe(
        &self,
        _user_id: &str,
        _resource_type: &str,
        _resource_key: &str,
        _owner_id: &str,
    ) -> bendclaw::types::Result<()> {
        Ok(())
    }

    async fn list(
        &self,
        _user_id: &str,
        _resource_type: &str,
    ) -> bendclaw::types::Result<Vec<Subscription>> {
        Ok(vec![])
    }

    async fn is_subscribed(
        &self,
        _user_id: &str,
        _resource_type: &str,
        _resource_key: &str,
        _owner_id: &str,
    ) -> bendclaw::types::Result<bool> {
        Ok(false)
    }
}

/// Usage sink that does nothing (for tests).
pub struct NoopUsageSink;

impl UsageSink for NoopUsageSink {
    fn touch_used(&self, _id: bendclaw::skills::definition::skill::SkillId, _agent_id: String) {}
}

/// Build a test `SkillProjector` backed by a temp directory (no DB needed for hub-only tests).
pub fn test_skill_projector(workspace_root: PathBuf) -> Arc<bendclaw::skills::sync::SkillIndex> {
    Arc::new(bendclaw::skills::sync::SkillIndex::new(
        workspace_root,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ))
}

/// Build a test `SkillWriter` wrapping a catalog with noop stores.
pub fn test_skill_service(projector: Arc<bendclaw::skills::sync::SkillIndex>) -> Arc<SkillWriter> {
    Arc::new(SkillWriter::new(
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        projector,
    ))
}

/// Build a test `SkillWriter` with a custom `SharedSkillStore`.
pub fn test_skill_service_with_store(
    store: Arc<dyn SharedSkillStore>,
    projector: Arc<bendclaw::skills::sync::SkillIndex>,
) -> Arc<SkillWriter> {
    Arc::new(SkillWriter::new(
        store,
        Arc::new(NoopSubscriptionStore),
        projector,
    ))
}
