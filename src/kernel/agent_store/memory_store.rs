//! Memory storage with FTS search.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use crate::base::Result;
use crate::observability::log::slog;
use crate::storage::cache::TtlCache;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const SEARCH_CACHE_TTL: Duration = Duration::from_secs(120);
const SEARCH_CACHE_CAPACITY: usize = 128;

/// Memory scope: user private, shared across users, or session temporary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    User,
    Shared,
    Session,
}

impl std::fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Shared => write!(f, "shared"),
            Self::Session => write!(f, "session"),
        }
    }
}

/// A memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub user_id: String,
    pub scope: MemoryScope,
    pub session_id: Option<String>,
    pub key: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Search result with relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResult {
    pub id: String,
    pub key: String,
    pub content: String,
    pub scope: MemoryScope,
    pub session_id: Option<String>,
    pub score: f32,
    pub updated_at: String,
}

/// Search options.
#[derive(Debug, Clone)]
pub struct SearchOpts {
    pub max_results: u32,
    pub include_shared: bool,
    pub session_id: Option<String>,
    pub min_score: f32,
}

impl Default for SearchOpts {
    fn default() -> Self {
        Self {
            max_results: 10,
            include_shared: true,
            session_id: None,
            min_score: 0.0,
        }
    }
}

pub fn parse_scope(scope: &str) -> MemoryScope {
    match scope {
        "shared" => MemoryScope::Shared,
        "session" => MemoryScope::Session,
        _ => MemoryScope::User,
    }
}

pub fn visibility_where(user_id: &str, include_shared: bool) -> String {
    if include_shared {
        format!(
            "(user_id = {} OR scope = {})",
            SqlVal::Str(user_id).render(),
            SqlVal::Str("shared").render()
        )
    } else {
        format!("user_id = {}", SqlVal::Str(user_id).render())
    }
}

pub fn build_search_extra_where(user_id: &str, opts: &SearchOpts) -> String {
    let mut clauses = vec![visibility_where(user_id, opts.include_shared)];
    if let Some(sid) = opts.session_id.as_deref() {
        clauses.push(format!("session_id = {}", SqlVal::Str(sid).render()));
    }
    if opts.min_score > 0.0 {
        clauses.push(format!("SCORE() >= {}", opts.min_score));
    }
    clauses.join(" AND ")
}

#[derive(Clone)]
struct EntryMapper;

impl RowMapper for EntryMapper {
    type Entity = MemoryEntry;

    fn columns(&self) -> &str {
        "id, user_id, scope, session_id, key, content, created_at, updated_at"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::base::Result<MemoryEntry> {
        Ok(MemoryEntry {
            id: sql::col(row, 0),
            user_id: sql::col(row, 1),
            scope: parse_scope(&sql::col(row, 2)),
            session_id: sql::col_opt(row, 3),
            key: sql::col(row, 4),
            content: sql::col(row, 5),
            created_at: sql::col(row, 6),
            updated_at: sql::col(row, 7),
        })
    }
}

#[derive(Clone)]
struct ResultMapper;

impl RowMapper for ResultMapper {
    type Entity = MemoryResult;

    fn columns(&self) -> &str {
        "id, key, content, scope, session_id, SCORE() AS score, updated_at"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::base::Result<MemoryResult> {
        Ok(MemoryResult {
            id: sql::col(row, 0),
            key: sql::col(row, 1),
            content: sql::col(row, 2),
            scope: parse_scope(&sql::col(row, 3)),
            session_id: sql::col_opt(row, 4),
            score: sql::col_f32(row, 5)?,
            updated_at: sql::col(row, 6),
        })
    }
}

/// Databend-backed memory store.
pub struct DatabendMemoryStore {
    entries: DatabendTable<EntryMapper>,
    search_table: DatabendTable<ResultMapper>,
    search_cache: Arc<TtlCache<Vec<MemoryResult>>>,
}

impl DatabendMemoryStore {
    pub fn new(pool: Pool) -> Self {
        Self {
            entries: DatabendTable::new(pool.clone(), "memories", EntryMapper),
            search_table: DatabendTable::new(pool, "memories", ResultMapper),
            search_cache: Arc::new(TtlCache::new(
                "memories_search",
                SEARCH_CACHE_CAPACITY,
                SEARCH_CACHE_TTL,
            )),
        }
    }

    pub async fn write(&self, user_id: &str, mut entry: MemoryEntry) -> Result<()> {
        entry.user_id = user_id.to_string();

        let scope_value = entry.scope.to_string();
        self.entries
            .insert(&[
                ("id", SqlVal::Str(&entry.id)),
                ("user_id", SqlVal::Str(&entry.user_id)),
                ("scope", SqlVal::Str(&scope_value)),
                (
                    "session_id",
                    SqlVal::str_or_null(entry.session_id.as_deref()),
                ),
                ("key", SqlVal::Str(&entry.key)),
                ("content", SqlVal::Str(&entry.content)),
                ("created_at", SqlVal::Raw("NOW()")),
                ("updated_at", SqlVal::Raw("NOW()")),
            ])
            .await
            .map_err(|e| {
                slog!(error, "memory", "write_failed", user_id, key = %entry.key, scope = %scope_value, error = %e,);
                e
            })?;
        self.search_cache.clear();

        Ok(())
    }

    pub async fn search(
        &self,
        query: &str,
        user_id: &str,
        opts: SearchOpts,
    ) -> Result<Vec<MemoryResult>> {
        let cache_key = format!(
            "{user_id}:{query}:{}:{}",
            opts.max_results, opts.include_shared
        );
        if let Some(cached) = self.search_cache.get(&cache_key) {
            return Ok(cached);
        }
        let extra = build_search_extra_where(user_id, &opts);

        let results = self
            .search_table
            .search_fts(
                "content",
                query,
                Some(&extra),
                "SCORE() DESC",
                opts.max_results as u64,
            )
            .await?
            .into_iter()
            .map(|(r, _)| r)
            .collect::<Vec<_>>();

        self.search_cache.put(cache_key, results.clone());
        slog!(
            info,
            "memory",
            "search",
            user_id,
            query,
            results = results.len(),
        );
        Ok(results)
    }

    pub async fn get(&self, user_id: &str, key: &str) -> Result<Option<MemoryEntry>> {
        let builder = crate::storage::sql::Sql::select(
            "id, user_id, scope, session_id, key, content, created_at, updated_at",
        )
        .from("memories")
        .where_raw(&visibility_where(user_id, true))
        .where_eq("key", SqlVal::Str(key))
        .order_by("updated_at DESC")
        .limit(1);
        let row = self.entries.pool().query_row(&builder.build()).await?;
        let result = row.as_ref().map(|r| EntryMapper.parse(r)).transpose()?;
        slog!(
            info,
            "memory",
            "get",
            user_id,
            key,
            found = result.is_some(),
        );
        Ok(result)
    }

    pub async fn get_by_id(&self, user_id: &str, id: &str) -> Result<Option<MemoryEntry>> {
        let builder = crate::storage::sql::Sql::select(
            "id, user_id, scope, session_id, key, content, created_at, updated_at",
        )
        .from("memories")
        .where_raw(&visibility_where(user_id, true))
        .where_eq("id", SqlVal::Str(id))
        .limit(1);
        let row = self.entries.pool().query_row(&builder.build()).await?;
        row.as_ref().map(|r| EntryMapper.parse(r)).transpose()
    }

    pub async fn delete(&self, user_id: &str, id: &str) -> Result<()> {
        self.entries
            .delete(&[
                Where("id", SqlVal::Str(id)),
                Where("user_id", SqlVal::Str(user_id)),
            ])
            .await
            .map_err(|e| {
                slog!(error, "memory", "delete_failed", user_id, id, error = %e,);
                e
            })?;
        self.search_cache.clear();

        Ok(())
    }

    pub async fn list(&self, user_id: &str, limit: u32) -> Result<Vec<MemoryEntry>> {
        let query = sql::Sql::select(
            "id, user_id, scope, session_id, key, content, created_at, updated_at",
        )
        .from("memories")
        .where_raw(&visibility_where(user_id, true))
        .order_by("updated_at DESC")
        .limit(limit as u64)
        .build();
        let rows = self.entries.pool().query_all(&query).await?;
        let results = rows
            .iter()
            .map(|r| EntryMapper.parse(r))
            .collect::<crate::base::Result<Vec<_>>>()?;

        Ok(results)
    }
}
