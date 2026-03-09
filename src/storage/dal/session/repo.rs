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

#[derive(Clone)]
struct SessionMapper;

impl RowMapper for SessionMapper {
    type Entity = SessionRecord;

    fn columns(&self) -> &str {
        "id, agent_id, user_id, title, PARSE_JSON(session_state), PARSE_JSON(meta), TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> SessionRecord {
        SessionRecord {
            id: sql::col(row, 0),
            agent_id: sql::col(row, 1),
            user_id: sql::col(row, 2),
            title: sql::col(row, 3),
            session_state: parse_variant_json(&sql::col(row, 4)),
            meta: parse_variant_json(&sql::col(row, 5)),
            created_at: sql::col(row, 6),
            updated_at: sql::col(row, 7),
        }
    }
}

#[derive(Clone)]
pub struct SessionRepo {
    table: DatabendTable<SessionMapper>,
}

impl SessionRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "sessions", SessionMapper),
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
}

fn parse_variant_json(raw: &str) -> serde_json::Value {
    if raw.trim().is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_str(raw).unwrap_or(serde_json::Value::Null)
}
