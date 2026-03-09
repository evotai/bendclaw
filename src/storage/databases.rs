//! Unified access to all agent databases.

use crate::base::sanitize_agent_id;
use crate::base::ErrorCode;
use crate::base::Result;
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
}

impl AgentDatabases {
    pub fn new(pool: Pool, prefix: &str) -> Result<Self> {
        validate_prefix(prefix)?;
        Ok(Self {
            pool,
            prefix: prefix.to_string(),
        })
    }

    /// Root pool (for CREATE DATABASE, migrations on the default DB, etc.).
    pub fn root_pool(&self) -> &Pool {
        &self.pool
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// List all agent IDs by scanning databases with the configured prefix.
    pub async fn list_agent_ids(&self) -> Result<Vec<String>> {
        let dbs = self.list_databases().await?;
        let mut ids: Vec<String> = dbs
            .iter()
            .filter_map(|db| db.strip_prefix(&self.prefix).map(String::from))
            .collect();
        ids.sort();
        Ok(ids)
    }

    /// List all agent database names.
    pub async fn list_databases(&self) -> Result<Vec<String>> {
        let sql = format!("SHOW DATABASES LIKE '{}%'", self.prefix);
        let rows = self.pool.query_all(&sql).await?;
        let dbs: Vec<String> = rows
            .iter()
            .map(|row| crate::storage::sql::col(row, 0))
            .collect();
        Ok(dbs)
    }

    /// Check if a specific agent database exists.
    pub async fn database_exists(&self, db_name: &str) -> Result<bool> {
        let sql = format!("SHOW DATABASES LIKE '{db_name}'");
        let rows = self.pool.query_all(&sql).await?;
        Ok(!rows.is_empty())
    }

    /// Database name for a given agent ID.
    pub fn agent_database_name(&self, agent_id: &str) -> String {
        let sanitized = sanitize_agent_id(agent_id);
        format!("{}{sanitized}", self.prefix)
    }

    /// Pool scoped to a specific agent's database.
    pub fn agent_pool(&self, agent_id: &str) -> Result<Pool> {
        let db_name = self.agent_database_name(agent_id);
        self.pool.with_database(&db_name)
    }
}
