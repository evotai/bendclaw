//! SkillManager — write-side orchestrator.
//!
//! Write: store + catalog.reconcile().
//! Usage tracking: store.
//! All reads go directly through SkillCatalog.

use std::sync::Arc;

use crate::base::Result;
use crate::kernel::skills::catalog::SkillCatalog;
use crate::kernel::skills::model::skill::Skill;
use crate::kernel::skills::model::skill::SkillId;
use crate::kernel::skills::runtime::UsageSink;
use crate::kernel::skills::shared::SharedSkillStore;
use crate::kernel::subscriptions::SubscriptionStore;

pub struct SkillManager {
    store: Arc<dyn SharedSkillStore>,
    sub_store: Arc<dyn SubscriptionStore>,
    catalog: Arc<SkillCatalog>,
}

impl SkillManager {
    pub fn new(
        store: Arc<dyn SharedSkillStore>,
        sub_store: Arc<dyn SubscriptionStore>,
        catalog: Arc<SkillCatalog>,
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
                warn, "skill_manager", "reconcile_after_create_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }

    pub async fn delete(&self, user_id: &str, name: &str) -> Result<()> {
        self.store.remove(user_id, name).await?;
        if let Err(e) = self.catalog.reconcile(user_id).await {
            crate::observability::log::slog!(
                warn, "skill_manager", "reconcile_after_delete_failed",
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
                warn, "skill_manager", "reconcile_after_subscribe_failed",
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
                warn, "skill_manager", "reconcile_after_unsubscribe_failed",
                user_id, error = %e,
            );
        }
        Ok(())
    }
}

impl UsageSink for SkillManager {
    fn touch_used(&self, id: SkillId, agent_id: String) {
        let store = self.store.clone();
        crate::base::spawn_fire_and_forget("skill_touch", async move {
            let _ = store.touch_last_used(&id, &agent_id).await;
        });
    }
}
