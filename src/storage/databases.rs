//! Unified access to all agent databases.

use std::time::Duration;

use crate::base::validate_agent_id;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::storage::cache::TtlCache;
use crate::storage::pool::Pool;

/// Only allow alphanumeric, underscore, and hyphen in DB prefix.
fn validate_prefix(prefix: &str) -> Result<()> {
    if prefix.is_empty() {
        return Err(ErrorCode::internal("database prefix must not be empty"));
    }
    if !prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ErrorCode::internal(format!(
            "database prefix contains invalid characters: '{prefix}' (only [a-zA-Z0-9_-] allowed)"
        )));
    }
    Ok(())
}

/// Manages discovery and access to all agent databases.
///
/// Each agent has its own database named `{prefix}{agent_id}`.
/// This struct provides a single entry point for listing, accessing,
/// and creating agent databases.
pub struct AgentDatabases {
    pool: Pool,
    prefix: String,
    agent_ids_cache: TtlCache<Vec<String>>,
}

impl AgentDatabases {
    pub fn new(pool: Pool, prefix: &str) -> Result<Self> {
        validate_prefix(prefix)?;
        Ok(Self {
            pool,
            prefix: prefix.to_string(),
            agent_ids_cache: TtlCache::new("agent_ids", 1, Duration::from_secs(30)),
        })
    }

    /// Root pool (for CREATE DATABASE, migrations on the default DB, etc.).
    pub fn root_pool(&self) -> &Pool {
        &self.pool
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// List all agent IDs from the evotai_meta registry.
    pub async fn list_agent_ids(&self) -> Result<Vec<String>> {
        if let Some(ids) = self.agent_ids_cache.get("all") {
            return Ok(ids);
        }
        let sql = "SELECT agent_id FROM evotai_meta.evotai_agents WHERE status = 'active' ORDER BY agent_id";
        let rows = self.pool.query_all(sql).await?;
        let mut ids: Vec<String> = rows
            .iter()
            .map(|row| crate::storage::sql::col(row, 0))
            .collect();
        ids.sort();
        self.agent_ids_cache.put("all".to_string(), ids.clone());
        Ok(ids)
    }

    /// Check if a specific agent database exists.
    pub async fn database_exists(&self, db_name: &str) -> Result<bool> {
        let sql = format!("SHOW DATABASES LIKE '{db_name}'");
        let rows = self.pool.query_all(&sql).await?;
        Ok(!rows.is_empty())
    }

    /// Database name for a given agent ID.
    pub fn agent_database_name(&self, agent_id: &str) -> Result<String> {
        validate_agent_id(agent_id)?;
        Ok(format!("{}{agent_id}", self.prefix))
    }

    /// Pool scoped to a specific agent's database.
    pub fn agent_pool(&self, agent_id: &str) -> Result<Pool> {
        let db_name = self.agent_database_name(agent_id)?;
        self.pool.with_database(&db_name)
    }
}
