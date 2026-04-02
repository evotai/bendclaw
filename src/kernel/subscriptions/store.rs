//! Subscription store — user opt-in to shared resources from others.
//!
//! Pure CRUD over `evotai_meta.resource_subscriptions`. No business logic.

use async_trait::async_trait;

use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::types::Result;

const TABLE: &str = "evotai_meta.resource_subscriptions";

// ── Types ──

#[derive(Debug, Clone)]
pub struct Subscription {
    pub user_id: String,
    pub resource_type: String,
    pub resource_key: String,
    pub owner_id: String,
    pub created_at: String,
}

// ── Trait ──

#[async_trait]
pub trait SubscriptionStore: Send + Sync {
    async fn subscribe(
        &self,
        user_id: &str,
        resource_type: &str,
        resource_key: &str,
        owner_id: &str,
    ) -> Result<()>;

    async fn unsubscribe(
        &self,
        user_id: &str,
        resource_type: &str,
        resource_key: &str,
        owner_id: &str,
    ) -> Result<()>;

    async fn list(&self, user_id: &str, resource_type: &str) -> Result<Vec<Subscription>>;

    async fn is_subscribed(
        &self,
        user_id: &str,
        resource_type: &str,
        resource_key: &str,
        owner_id: &str,
    ) -> Result<bool>;
}

// ── Implementation ──

pub struct SharedSubscriptionStore {
    pool: Pool,
}

impl SharedSubscriptionStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub fn noop() -> Self {
        Self { pool: Pool::noop() }
    }
}

#[async_trait]
impl SubscriptionStore for SharedSubscriptionStore {
    async fn subscribe(
        &self,
        user_id: &str,
        resource_type: &str,
        resource_key: &str,
        owner_id: &str,
    ) -> Result<()> {
        // Idempotent: delete then insert
        let del = format!(
            "DELETE FROM {TABLE} WHERE user_id = {} AND resource_type = {} AND resource_key = {} AND owner_id = {}",
            SqlVal::Str(user_id).render(),
            SqlVal::Str(resource_type).render(),
            SqlVal::Str(resource_key).render(),
            SqlVal::Str(owner_id).render(),
        );
        let _ = self.pool.exec(&del).await;

        let stmt = format!(
            "INSERT INTO {TABLE} (user_id, resource_type, resource_key, owner_id, created_at) \
             VALUES ({}, {}, {}, {}, NOW())",
            SqlVal::Str(user_id).render(),
            SqlVal::Str(resource_type).render(),
            SqlVal::Str(resource_key).render(),
            SqlVal::Str(owner_id).render(),
        );
        self.pool.exec(&stmt).await
    }

    async fn unsubscribe(
        &self,
        user_id: &str,
        resource_type: &str,
        resource_key: &str,
        owner_id: &str,
    ) -> Result<()> {
        let stmt = format!(
            "DELETE FROM {TABLE} WHERE user_id = {} AND resource_type = {} AND resource_key = {} AND owner_id = {}",
            SqlVal::Str(user_id).render(),
            SqlVal::Str(resource_type).render(),
            SqlVal::Str(resource_key).render(),
            SqlVal::Str(owner_id).render(),
        );
        self.pool.exec(&stmt).await
    }

    async fn list(&self, user_id: &str, resource_type: &str) -> Result<Vec<Subscription>> {
        let stmt = format!(
            "SELECT user_id, resource_type, resource_key, owner_id, TO_VARCHAR(created_at) \
             FROM {TABLE} WHERE user_id = {} AND resource_type = {} ORDER BY created_at DESC",
            SqlVal::Str(user_id).render(),
            SqlVal::Str(resource_type).render(),
        );
        let rows = self.pool.query_all(&stmt).await?;
        Ok(rows
            .iter()
            .filter_map(|r| parse_subscription(r).ok())
            .collect())
    }

    async fn is_subscribed(
        &self,
        user_id: &str,
        resource_type: &str,
        resource_key: &str,
        owner_id: &str,
    ) -> Result<bool> {
        let stmt = format!(
            "SELECT 1 FROM {TABLE} WHERE user_id = {} AND resource_type = {} AND resource_key = {} AND owner_id = {} LIMIT 1",
            SqlVal::Str(user_id).render(),
            SqlVal::Str(resource_type).render(),
            SqlVal::Str(resource_key).render(),
            SqlVal::Str(owner_id).render(),
        );
        let row = self.pool.query_row(&stmt).await?;
        Ok(row.is_some())
    }
}

fn parse_subscription(row: &serde_json::Value) -> Result<Subscription> {
    Ok(Subscription {
        user_id: sql::col(row, 0),
        resource_type: sql::col(row, 1),
        resource_key: sql::col(row, 2),
        owner_id: sql::col(row, 3),
        created_at: sql::col(row, 4),
    })
}
