use crate::storage::pool::Pool;

/// Tracks secret variable usage (last-used timestamps).
/// Injected into core tools at registration time so they don't need pool directly.
pub trait SecretUsageSink: Send + Sync {
    fn touch_last_used(&self, id: &str, agent_id: &str);
    fn touch_last_used_many(&self, ids: &[String], agent_id: &str);
}

/// Real implementation backed by Databend pool.
pub struct DbSecretUsageSink {
    pool: Pool,
}

impl DbSecretUsageSink {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl SecretUsageSink for DbSecretUsageSink {
    fn touch_last_used(&self, id: &str, agent_id: &str) {
        let pool = self.pool.clone();
        let id = id.to_string();
        let agent_id = agent_id.to_string();
        crate::base::spawn_fire_and_forget("variable_touch_last_used", async move {
            let store = crate::kernel::variables::store::SharedVariableStore::new(pool);
            use crate::kernel::variables::store::VariableStore;
            let _ = store.touch_last_used(&id, &agent_id).await;
        });
    }

    fn touch_last_used_many(&self, ids: &[String], agent_id: &str) {
        if ids.is_empty() {
            return;
        }
        let pool = self.pool.clone();
        let ids = ids.to_vec();
        let agent_id = agent_id.to_string();
        crate::base::spawn_fire_and_forget("variable_touch_last_used", async move {
            let store = crate::kernel::variables::store::SharedVariableStore::new(pool);
            use crate::kernel::variables::store::VariableStore;
            let _ = store.touch_last_used_many(&ids, &agent_id).await;
        });
    }
}

/// No-op implementation for ephemeral sessions.
pub struct NoopSecretUsageSink;

impl SecretUsageSink for NoopSecretUsageSink {
    fn touch_last_used(&self, _id: &str, _agent_id: &str) {}
    fn touch_last_used_many(&self, _ids: &[String], _agent_id: &str) {}
}
