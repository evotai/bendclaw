//! Shared memory store — IO layer.
//!
//! Pure CRUD + FTS over `evotai_meta.memory`. No business logic.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use crate::storage::cache::TtlCache;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::types::Result;

const SEARCH_CACHE_TTL: Duration = Duration::from_secs(120);
const SEARCH_CACHE_CAPACITY: usize = 128;
const TABLE: &str = "evotai_meta.memory";

// ── Types ──

/// Memory visibility scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    /// Visible only to the agent that created it.
    Agent,
    /// Visible to all agents for the same user.
    Shared,
}

impl std::fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Agent => write!(f, "agent"),
            Self::Shared => write!(f, "shared"),
        }
    }
}

pub fn parse_scope(s: &str) -> MemoryScope {
    match s {
        "shared" => MemoryScope::Shared,
        _ => MemoryScope::Agent,
    }
}

/// A memory entry (pure data, no kernel dependencies).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub user_id: String,
    pub agent_id: String,
    pub scope: MemoryScope,
    pub key: String,
    pub content: String,
    pub access_count: u32,
    pub last_accessed_at: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Search result with relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub id: String,
    pub key: String,
    pub content: String,
    pub scope: MemoryScope,
    pub agent_id: String,
    pub score: f32,
    pub access_count: u32,
    pub updated_at: String,
}

// ── Trait ──

/// IO-layer trait. Pure data access, no business logic.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn write(&self, entry: &MemoryEntry) -> Result<()>;
    async fn search(
        &self,
        query: &str,
        user_id: &str,
        agent_id: &str,
        limit: u32,
    ) -> Result<Vec<MemorySearchResult>>;
    async fn get(&self, user_id: &str, agent_id: &str, key: &str) -> Result<Option<MemoryEntry>>;
    async fn get_by_id(&self, user_id: &str, id: &str) -> Result<Option<MemoryEntry>>;
    async fn delete(&self, user_id: &str, id: &str) -> Result<()>;
    async fn list(&self, user_id: &str, agent_id: &str, limit: u32) -> Result<Vec<MemoryEntry>>;
    async fn touch(&self, user_id: &str, id: &str) -> Result<()>;
    async fn prune(&self, user_id: &str, max_age_days: u32, min_access: u32) -> Result<usize>;
}

// ── Helpers ──

/// Visibility filter: own agent memories + all shared memories for the user.
fn visibility_where(user_id: &str, agent_id: &str) -> String {
    format!(
        "(user_id = {} AND (agent_id = {} OR scope = {}))",
        SqlVal::Str(user_id).render(),
        SqlVal::Str(agent_id).render(),
        SqlVal::Str("shared").render(),
    )
}

// ── Implementation ──

/// Databend-backed shared memory store operating on `evotai_meta.memory`.
pub struct SharedMemoryStore {
    pool: Pool,
    search_cache: Arc<TtlCache<Vec<MemorySearchResult>>>,
}

impl SharedMemoryStore {
    pub fn new(pool: Pool) -> Self {
        Self {
            pool,
            search_cache: Arc::new(TtlCache::new(
                "shared_memory_search",
                SEARCH_CACHE_CAPACITY,
                SEARCH_CACHE_TTL,
            )),
        }
    }
}

#[async_trait]
impl MemoryStore for SharedMemoryStore {
    async fn write(&self, entry: &MemoryEntry) -> Result<()> {
        let scope_str = entry.scope.to_string();
        let cols = "id, user_id, agent_id, scope, key, content, access_count, last_accessed_at, created_at, updated_at";
        let vals = format!(
            "{}, {}, {}, {}, {}, {}, 0, NOW(), NOW(), NOW()",
            SqlVal::Str(&entry.id).render(),
            SqlVal::Str(&entry.user_id).render(),
            SqlVal::Str(&entry.agent_id).render(),
            SqlVal::Str(&scope_str).render(),
            SqlVal::Str(&entry.key).render(),
            SqlVal::Str(&entry.content).render(),
        );
        let stmt = format!("INSERT INTO {TABLE} ({cols}) VALUES ({vals})");
        self.pool.exec(&stmt).await?;
        self.search_cache.clear();
        Ok(())
    }

    async fn search(
        &self,
        query: &str,
        user_id: &str,
        agent_id: &str,
        limit: u32,
    ) -> Result<Vec<MemorySearchResult>> {
        let cache_key = format!("{user_id}:{agent_id}:{query}:{limit}");
        if let Some(cached) = self.search_cache.get(&cache_key) {
            return Ok(cached);
        }

        let vis = visibility_where(user_id, agent_id);
        let stmt = format!(
            "SELECT id, key, content, scope, agent_id, SCORE() AS score, access_count, updated_at \
             FROM {TABLE} \
             WHERE QUERY(content, {}) AND {vis} \
             ORDER BY SCORE() DESC \
             LIMIT {limit}",
            SqlVal::Str(query).render(),
        );
        let rows = self.pool.query_all(&stmt).await?;
        let results: Vec<MemorySearchResult> = rows
            .iter()
            .filter_map(|row| parse_search_result(row).ok())
            .collect();

        self.search_cache.put(cache_key, results.clone());
        Ok(results)
    }

    async fn get(&self, user_id: &str, agent_id: &str, key: &str) -> Result<Option<MemoryEntry>> {
        let vis = visibility_where(user_id, agent_id);
        let stmt = format!(
            "SELECT {ENTRY_COLS} FROM {TABLE} \
             WHERE {vis} AND key = {} \
             ORDER BY updated_at DESC LIMIT 1",
            SqlVal::Str(key).render(),
        );
        let row = self.pool.query_row(&stmt).await?;
        row.as_ref().map(parse_entry).transpose()
    }

    async fn get_by_id(&self, user_id: &str, id: &str) -> Result<Option<MemoryEntry>> {
        let stmt = format!(
            "SELECT {ENTRY_COLS} FROM {TABLE} \
             WHERE user_id = {} AND id = {} LIMIT 1",
            SqlVal::Str(user_id).render(),
            SqlVal::Str(id).render(),
        );
        let row = self.pool.query_row(&stmt).await?;
        row.as_ref().map(parse_entry).transpose()
    }

    async fn delete(&self, user_id: &str, id: &str) -> Result<()> {
        let stmt = format!(
            "DELETE FROM {TABLE} WHERE id = {} AND user_id = {}",
            SqlVal::Str(id).render(),
            SqlVal::Str(user_id).render(),
        );
        self.pool.exec(&stmt).await?;
        self.search_cache.clear();
        Ok(())
    }

    async fn list(&self, user_id: &str, agent_id: &str, limit: u32) -> Result<Vec<MemoryEntry>> {
        let vis = visibility_where(user_id, agent_id);
        let stmt = format!(
            "SELECT {ENTRY_COLS} FROM {TABLE} \
             WHERE {vis} ORDER BY updated_at DESC LIMIT {limit}",
        );
        let rows = self.pool.query_all(&stmt).await?;
        rows.iter().map(parse_entry).collect()
    }

    async fn touch(&self, user_id: &str, id: &str) -> Result<()> {
        let stmt = format!(
            "UPDATE {TABLE} SET access_count = access_count + 1, last_accessed_at = NOW() \
             WHERE id = {} AND user_id = {}",
            SqlVal::Str(id).render(),
            SqlVal::Str(user_id).render(),
        );
        self.pool.exec(&stmt).await?;
        Ok(())
    }

    async fn prune(&self, user_id: &str, max_age_days: u32, min_access: u32) -> Result<usize> {
        let stmt = format!(
            "DELETE FROM {TABLE} \
             WHERE user_id = {} \
               AND access_count < {min_access} \
               AND last_accessed_at < NOW() - INTERVAL {max_age_days} DAY \
               AND scope != 'shared'",
            SqlVal::Str(user_id).render(),
        );
        self.pool.exec(&stmt).await?;
        // Databend doesn't return affected rows easily; return 0 as placeholder.
        Ok(0)
    }
}

// ── Row parsers ──

const ENTRY_COLS: &str =
    "id, user_id, agent_id, scope, key, content, access_count, last_accessed_at, created_at, updated_at";

fn parse_entry(row: &serde_json::Value) -> Result<MemoryEntry> {
    Ok(MemoryEntry {
        id: sql::col(row, 0),
        user_id: sql::col(row, 1),
        agent_id: sql::col(row, 2),
        scope: parse_scope(&sql::col(row, 3)),
        key: sql::col(row, 4),
        content: sql::col(row, 5),
        access_count: sql::col(row, 6).parse::<u32>().unwrap_or(0),
        last_accessed_at: sql::col(row, 7),
        created_at: sql::col(row, 8),
        updated_at: sql::col(row, 9),
    })
}

fn parse_search_result(row: &serde_json::Value) -> Result<MemorySearchResult> {
    Ok(MemorySearchResult {
        id: sql::col(row, 0),
        key: sql::col(row, 1),
        content: sql::col(row, 2),
        scope: parse_scope(&sql::col(row, 3)),
        agent_id: sql::col(row, 4),
        score: sql::col_f32(row, 5)?,
        access_count: sql::col(row, 6).parse::<u32>().unwrap_or(0),
        updated_at: sql::col(row, 7),
    })
}
