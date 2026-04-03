//! VariableService — business logic facade for variables.

use std::sync::Arc;

use crate::subscriptions::SubscriptionStore;
use crate::types::Result;
use crate::variables::store::Variable;
use crate::variables::store::VariableStore;

pub struct VariableService {
    store: Arc<dyn VariableStore>,
    sub_store: Arc<dyn SubscriptionStore>,
}

impl VariableService {
    pub fn new(store: Arc<dyn VariableStore>, sub_store: Arc<dyn SubscriptionStore>) -> Self {
        Self { store, sub_store }
    }

    pub async fn list_active(&self, user_id: &str) -> Result<Vec<Variable>> {
        self.store.list_active(user_id, 10_000).await
    }

    pub async fn get(&self, user_id: &str, id: &str) -> Result<Option<Variable>> {
        self.store.get(user_id, id).await
    }

    pub async fn create(&self, variable: Variable) -> Result<()> {
        self.store.insert(&variable).await
    }

    pub async fn update(
        &self,
        user_id: &str,
        id: &str,
        key: &str,
        value: &str,
        secret: bool,
        revoked: bool,
    ) -> Result<()> {
        self.store
            .update(user_id, id, key, value, secret, revoked)
            .await
    }

    pub async fn delete(&self, user_id: &str, id: &str) -> Result<()> {
        self.store.delete(user_id, id).await
    }

    pub async fn list_all(&self, user_id: &str) -> Result<Vec<Variable>> {
        self.store.list_all(user_id, 10_000).await
    }

    /// Browse all shared variables from other users (including unsubscribed).
    pub async fn list_shared(&self, user_id: &str) -> Result<Vec<Variable>> {
        self.store.list_shared(user_id, 10_000).await
    }

    pub async fn subscribe(&self, user_id: &str, variable_id: &str, owner_id: &str) -> Result<()> {
        self.sub_store
            .subscribe(user_id, "variable", variable_id, owner_id)
            .await
    }

    pub async fn unsubscribe(
        &self,
        user_id: &str,
        variable_id: &str,
        owner_id: &str,
    ) -> Result<()> {
        self.sub_store
            .unsubscribe(user_id, "variable", variable_id, owner_id)
            .await
    }

    /// Fire-and-forget touch for variable usage tracking.
    pub fn touch_used(&self, ids: Vec<String>, agent_id: String) {
        let store = self.store.clone();
        crate::types::spawn_fire_and_forget("variable_touch", async move {
            let _ = store.touch_last_used_many(&ids, &agent_id).await;
        });
    }
}
