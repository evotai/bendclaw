use std::time::Duration;

use super::record::SessionRecord;
use crate::storage::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;
use crate::types::ErrorCode;
use crate::types::Result;

const REPO: &str = "sessions";
const CACHE_TTL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct SessionWrite {
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub title: String,
    pub base_key: String,
    pub replaced_by_session_id: String,
    pub reset_reason: String,
    pub session_state: serde_json::Value,
    pub meta: serde_json::Value,
}

impl SessionWrite {
    pub fn from_record(record: &SessionRecord) -> Self {
        Self {
            session_id: record.id.clone(),
            agent_id: record.agent_id.clone(),
            user_id: record.user_id.clone(),
            title: record.title.clone(),
            base_key: record.base_key.clone(),
            replaced_by_session_id: record.replaced_by_session_id.clone(),
            reset_reason: record.reset_reason.clone(),
            session_state: record.session_state.clone(),
            meta: record.meta.clone(),
        }
    }
}

#[derive(Clone)]
struct SessionMapper;

impl RowMapper for SessionMapper {
    type Entity = SessionRecord;

    fn columns(&self) -> &str {
        "id, agent_id, user_id, title, scope, base_key, replaced_by_session_id, reset_reason, PARSE_JSON(session_state), PARSE_JSON(meta), TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::types::Result<SessionRecord> {
        Ok(SessionRecord {
            id: sql::col(row, 0),
            agent_id: sql::col(row, 1),
            user_id: sql::col(row, 2),
            title: sql::col(row, 3),
            scope: sql::col(row, 4),
            base_key: sql::col(row, 5),
            replaced_by_session_id: sql::col(row, 6),
            reset_reason: sql::col(row, 7),
            session_state: parse_variant_json(&sql::col(row, 8))?,
            meta: parse_variant_json(&sql::col(row, 9))?,
            created_at: sql::col(row, 10),
            updated_at: sql::col(row, 11),
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

    pub async fn upsert(&self, input: SessionWrite) -> Result<()> {
        let state_json = serde_json::to_string(&input.session_state)
            .map_err(|e| ErrorCode::storage_serde(format!("serialize session_state: {e}")))?;
        let state_expr = format!("PARSE_JSON('{}')", sql::escape(&state_json));

        let meta_json = serde_json::to_string(&input.meta)
            .map_err(|e| ErrorCode::storage_serde(format!("serialize session meta: {e}")))?;
        let meta_expr = format!("PARSE_JSON('{}')", sql::escape(&meta_json));

        let result = self
            .table
            .upsert(
                &[
                    ("id", SqlVal::Str(&input.session_id)),
                    ("agent_id", SqlVal::Str(&input.agent_id)),
                    ("user_id", SqlVal::Str(&input.user_id)),
                    ("title", SqlVal::Str(&input.title)),
                    ("scope", SqlVal::Str("private")),
                    ("base_key", SqlVal::Str(&input.base_key)),
                    (
                        "replaced_by_session_id",
                        SqlVal::Str(&input.replaced_by_session_id),
                    ),
                    ("reset_reason", SqlVal::Str(&input.reset_reason)),
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
                serde_json::json!({
                    "session_id": input.session_id,
                    "agent_id": input.agent_id
                }),
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

    pub async fn mark_replaced(
        &self,
        session_id: &str,
        replaced_by_session_id: &str,
        reset_reason: &str,
    ) -> Result<()> {
        let sql = format!(
            "UPDATE sessions SET replaced_by_session_id = '{}', reset_reason = '{}', updated_at = NOW() WHERE id = '{}'",
            sql::escape(replaced_by_session_id),
            sql::escape(reset_reason),
            sql::escape(session_id)
        );
        let result = self.table.exec_raw(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "mark_replaced",
                serde_json::json!({
                    "session_id": session_id,
                    "replaced_by_session_id": replaced_by_session_id,
                    "reset_reason": reset_reason
                }),
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

    pub async fn load_active_by_base_key(&self, base_key: &str) -> Result<Option<SessionRecord>> {
        let result = async {
            let query = format!(
                "SELECT {} FROM sessions WHERE base_key = '{}' AND replaced_by_session_id = '' ORDER BY created_at DESC LIMIT 1",
                SessionMapper.columns(),
                sql::escape(base_key),
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
                "load_active_by_base_key",
                serde_json::json!({"base_key": base_key}),
                error,
            );
        }
        result
    }
}

fn parse_variant_json(raw: &str) -> crate::types::Result<serde_json::Value> {
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
