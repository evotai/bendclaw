//! SkillWriter — write-side orchestrator.
//!
//! Write: store + catalog.reconcile().
//! Usage tracking: store.
//! All reads go directly through SkillIndex.

use std::sync::Arc;

use crate::execution::skills::UsageSink;
use crate::kernel::subscriptions::SubscriptionStore;
use crate::skills::definition::skill::Skill;
use crate::skills::definition::skill::SkillId;
use crate::skills::store::SharedSkillStore;
use crate::skills::sync::SkillIndex;
use crate::types::Result;

pub struct SkillWriter {
    store: Arc<dyn SharedSkillStore>,
    sub_store: Arc<dyn SubscriptionStore>,
    catalog: Arc<SkillIndex>,
}

impl SkillWriter {
    pub fn new(
        store: Arc<dyn SharedSkillStore>,
        sub_store: Arc<dyn SubscriptionStore>,
        catalog: Arc<SkillIndex>,
    ) -> Self {
        Self {
            store,
            sub_store,
            catalog,
        }
    }

    // ── Write: DB + reconcile ───────────────────────────────────────────

    pub async fn create(&self, user_id: &str, skill: Skill) -> Result<()> {
        skill.validate()?;
        self.store.save(user_id, &skill).await?;
        if let Err(e) = self.catalog.reconcile(user_id).await {
            crate::observability::log::slog!(
                warn, "skill_writer", "reconcile_after_create_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }

    pub async fn delete(&self, user_id: &str, name: &str) -> Result<()> {
        self.store.remove(user_id, name).await?;
        if let Err(e) = self.catalog.reconcile(user_id).await {
            crate::observability::log::slog!(
                warn, "skill_writer", "reconcile_after_delete_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }

    // ── Shared browsing + subscriptions ─────────────────────────────────

    pub async fn list_shared(&self, user_id: &str) -> Result<Vec<Skill>> {
        self.store.list_shared(user_id).await
    }

    pub async fn subscribe(&self, user_id: &str, skill_name: &str, owner_id: &str) -> Result<()> {
        self.sub_store
            .subscribe(user_id, "skill", skill_name, owner_id)
            .await?;
        if let Err(e) = self.catalog.reconcile(user_id).await {
            crate::observability::log::slog!(
                warn, "skill_writer", "reconcile_after_subscribe_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }

    pub async fn unsubscribe(&self, user_id: &str, skill_name: &str, owner_id: &str) -> Result<()> {
        self.sub_store
            .unsubscribe(user_id, "skill", skill_name, owner_id)
            .await?;
        if let Err(e) = self.catalog.reconcile(user_id).await {
            crate::observability::log::slog!(
                warn, "skill_writer", "reconcile_after_unsubscribe_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }
}

impl UsageSink for SkillWriter {
    fn touch_used(&self, id: SkillId, agent_id: String) {
        let store = self.store.clone();
        crate::types::spawn_fire_and_forget("skill_touch", async move {
            let _ = store.touch_last_used(&id, &agent_id).await;
        });
    }
}
