use std::time::Duration;

use super::record::SessionRecord;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const REPO: &str = "sessions";
const CACHE_TTL: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct SessionMapper;

impl RowMapper for SessionMapper {
    type Entity = SessionRecord;

    fn columns(&self) -> &str {
        "id, agent_id, user_id, title, scope, PARSE_JSON(session_state), PARSE_JSON(meta), TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::base::Result<SessionRecord> {
        Ok(SessionRecord {
            id: sql::col(row, 0),
            agent_id: sql::col(row, 1),
            user_id: sql::col(row, 2),
            title: sql::col(row, 3),
            scope: sql::col(row, 4),
            session_state: parse_variant_json(&sql::col(row, 5))?,
            meta: parse_variant_json(&sql::col(row, 6))?,
            created_at: sql::col(row, 7),
            updated_at: sql::col(row, 8),
        })
    }
}

#[derive(Clone)]
pub struct SessionRepo {
    table: DatabendTable<SessionMapper>,
}

impl SessionRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "sessions", SessionMapper).with_ttl_cache(CACHE_TTL),
        }
    }

    pub async fn upsert(
        &self,
        session_id: &str,
        agent_id: &str,
        user_id: &str,
        title: Option<&str>,
        session_state: Option<&serde_json::Value>,
        meta: Option<&serde_json::Value>,
    ) -> Result<()> {
        let state_json =
            serde_json::to_string(session_state.unwrap_or(&serde_json::Value::Null))
                .map_err(|e| ErrorCode::storage_serde(format!("serialize session_state: {e}")))?;
        let state_expr = format!("PARSE_JSON('{}')", sql::escape(&state_json));

        let meta_json = serde_json::to_string(meta.unwrap_or(&serde_json::Value::Null))
            .map_err(|e| ErrorCode::storage_serde(format!("serialize session meta: {e}")))?;
        let meta_expr = format!("PARSE_JSON('{}')", sql::escape(&meta_json));

        let result = self
            .table
            .upsert(
                &[
                    ("id", SqlVal::Str(session_id)),
                    ("agent_id", SqlVal::Str(agent_id)),
                    ("user_id", SqlVal::Str(user_id)),
                    ("title", SqlVal::Str(title.unwrap_or_default())),
                    ("scope", SqlVal::Str("private")),
                    ("session_state", SqlVal::Raw(&state_expr)),
                    ("meta", SqlVal::Raw(&meta_expr)),
                    ("created_at", SqlVal::Raw("NOW()")),
                    ("updated_at", SqlVal::Raw("NOW()")),
                ],
                "id",
            )
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "upsert",
                serde_json::json!({"session_id": session_id, "agent_id": agent_id}),
                error,
            );
        }
        result
    }

    pub async fn load(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        let result = self
            .table
            .get(&[Where("id", SqlVal::Str(session_id))])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "load",
                serde_json::json!({"session_id": session_id}),
                error,
            );
        }
        result
    }

    pub async fn update_state(&self, session_id: &str, state: &serde_json::Value) -> Result<()> {
        let json = serde_json::to_string(state)
            .map_err(|e| ErrorCode::storage_serde(format!("serialize session_state: {e}")))?;
        let sql = format!(
            "UPDATE sessions SET session_state = PARSE_JSON('{}'), updated_at = NOW() WHERE id = '{}'",
            sql::escape(&json),
            sql::escape(session_id)
        );
        let result = self.table.exec_raw(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "update_state",
                serde_json::json!({"session_id": session_id}),
                error,
            );
        }
        result
    }

    pub async fn delete_by_id(&self, session_id: &str) -> Result<()> {
        let sql = format!(
            "DELETE FROM sessions WHERE id = '{}'",
            sql::escape(session_id)
        );
        let result = self.table.exec_raw(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "delete_by_id",
                serde_json::json!({"session_id": session_id}),
                error,
            );
        }
        result
    }

    pub async fn count_by_user_search(&self, user_id: &str, search: Option<&str>) -> Result<u64> {
        let condition = user_search_condition(user_id, search);
        let result = async {
            let query = format!("SELECT COUNT(*) FROM sessions WHERE {condition}");
            let row = self.table.pool().query_row(&query).await?;
            sql::agg_u64_or_zero(row.as_ref(), 0)
        }
        .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "count_by_user_search",
                serde_json::json!({"user_id": user_id, "search": search}),
                error,
            );
        }
        result
    }

    pub async fn list_by_user_search(
        &self,
        user_id: &str,
        search: Option<&str>,
        order: &str,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<SessionRecord>> {
        let condition = user_search_condition(user_id, search);
        let result = async {
            let query = format!(
                "SELECT {} FROM sessions WHERE {condition} ORDER BY {order} LIMIT {limit} OFFSET {offset}",
                SessionMapper.columns()
            );
            let rows = self.table.pool().query_all(&query).await?;
            rows.iter().map(|row| SessionMapper.parse(row)).collect()
        }
        .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_by_user_search",
                serde_json::json!({
                    "user_id": user_id,
                    "search": search,
                    "order": order,
                    "limit": limit,
                    "offset": offset
                }),
                error,
            );
        }
        result
    }

    pub async fn list_by_user(&self, user_id: &str, limit: u32) -> Result<Vec<SessionRecord>> {
        let result = self
            .table
            .list(
                &[Where("user_id", SqlVal::Str(user_id))],
                "updated_at DESC",
                limit as u64,
            )
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_by_user",
                serde_json::json!({"user_id": user_id, "limit": limit}),
                error,
            );
        }
        result
    }

    /// Find the most recent session matching a base key (exact or with `#timestamp` suffix).
    pub async fn latest_by_prefix(&self, prefix: &str) -> Result<Option<SessionRecord>> {
        let result = async {
            let query = format!(
                "SELECT {} FROM sessions WHERE id = '{}' OR id LIKE '{}#%' ORDER BY created_at DESC LIMIT 1",
                SessionMapper.columns(),
                sql::escape(prefix),
                sql::escape_like(prefix),
            );
            let rows = self.table.pool().query_all(&query).await?;
            match rows.first() {
                Some(row) => Ok(Some(SessionMapper.parse(row)?)),
                None => Ok(None),
            }
        }
        .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "latest_by_prefix",
                serde_json::json!({"prefix": prefix}),
                error,
            );
        }
        result
    }
}

fn parse_variant_json(raw: &str) -> crate::base::Result<serde_json::Value> {
    if raw.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    sql::parse_json(raw, "sessions.variant")
}

fn user_search_condition(user_id: &str, search: Option<&str>) -> String {
    let mut condition = format!("user_id = '{}'", sql::escape(user_id));
    if let Some(search) = search {
        let search = sql::escape_like(search);
        condition.push_str(&format!(" AND title LIKE '%{search}%' ESCAPE '^'"));
    }
    condition
}
