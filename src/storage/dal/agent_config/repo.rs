use std::collections::HashMap;

use super::record::AgentConfigRecord;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const REPO: &str = "agent_config";

#[derive(Clone)]
struct ConfigMapper;

impl RowMapper for ConfigMapper {
    type Entity = AgentConfigRecord;

    fn columns(&self) -> &str {
        "agent_id, system_prompt, display_name, description, \
         identity, soul, token_limit_total, token_limit_daily, \
         PARSE_JSON(env), TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> AgentConfigRecord {
        AgentConfigRecord {
            agent_id: sql::col(row, 0),
            system_prompt: sql::col(row, 1),
            display_name: sql::col(row, 2),
            description: sql::col(row, 3),
            identity: sql::col(row, 4),
            soul: sql::col(row, 5),
            token_limit_total: parse_optional_u64(&sql::col(row, 6)),
            token_limit_daily: parse_optional_u64(&sql::col(row, 7)),
            env: parse_env_json(&sql::col(row, 8)),
            created_at: sql::col(row, 9),
            updated_at: sql::col(row, 10),
        }
    }
}

#[derive(Clone)]
pub struct AgentConfigStore {
    table: DatabendTable<ConfigMapper>,
}

impl AgentConfigStore {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "agent_config", ConfigMapper),
        }
    }

    pub async fn get(&self, agent_id: &str) -> Result<Option<AgentConfigRecord>> {
        let result = self
            .table
            .get(&[Where("agent_id", SqlVal::Str(agent_id))])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "get",
                serde_json::json!({"agent_id": agent_id}),
                error,
            );
        }
        result
    }

    pub async fn get_any(&self) -> Result<Option<AgentConfigRecord>> {
        let result = self.table.get(&[]).await;
        if let Err(error) = &result {
            repo_error(REPO, "get_any", serde_json::json!({}), error);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert(
        &self,
        agent_id: &str,
        system_prompt: Option<&str>,
        display_name: Option<&str>,
        description: Option<&str>,
        identity: Option<&str>,
        soul: Option<&str>,
        token_limit_total: Option<Option<u64>>,
        token_limit_daily: Option<Option<u64>>,
        env: Option<&HashMap<String, String>>,
    ) -> Result<()> {
        let env_json = match env {
            Some(e) => serde_json::to_string(e).map_err(|e| {
                ErrorCode::storage_serde(format!("serialize agent_config env: {e}"))
            })?,
            None => "{}".to_string(),
        };
        let env_expr = format!("PARSE_JSON('{}')", sql::escape(&env_json));

        let total_expr = match token_limit_total {
            Some(Some(v)) => SqlVal::Raw(&format!("{v}")),
            Some(None) => SqlVal::Raw("NULL"),
            None => SqlVal::Raw("NULL"),
        };
        let total_str = match token_limit_total {
            Some(Some(v)) => format!("{v}"),
            _ => "NULL".to_string(),
        };
        let daily_str = match token_limit_daily {
            Some(Some(v)) => format!("{v}"),
            _ => "NULL".to_string(),
        };
        let daily_expr = match token_limit_daily {
            Some(Some(v)) => SqlVal::Raw(&format!("{v}")),
            Some(None) => SqlVal::Raw("NULL"),
            None => SqlVal::Raw("NULL"),
        };
        // Workaround: we need stable references for SqlVal::Raw
        let _ = (total_expr, daily_expr);

        let result = self
            .table
            .upsert(
                &[
                    ("agent_id", SqlVal::Str(agent_id)),
                    ("system_prompt", SqlVal::Str(system_prompt.unwrap_or(""))),
                    ("display_name", SqlVal::Str(display_name.unwrap_or(""))),
                    ("description", SqlVal::Str(description.unwrap_or(""))),
                    ("identity", SqlVal::Str(identity.unwrap_or(""))),
                    ("soul", SqlVal::Str(soul.unwrap_or(""))),
                    ("token_limit_total", SqlVal::Raw(&total_str)),
                    ("token_limit_daily", SqlVal::Raw(&daily_str)),
                    ("env", SqlVal::Raw(&env_expr)),
                    ("created_at", SqlVal::Raw("NOW()")),
                    ("updated_at", SqlVal::Raw("NOW()")),
                ],
                "agent_id",
            )
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "upsert",
                serde_json::json!({"agent_id": agent_id}),
                error,
            );
        }
        result
    }

    pub async fn set_env_var(&self, agent_id: &str, key: &str, value: &str) -> Result<()> {
        let sql = format!(
            "UPDATE agent_config SET env = OBJECT_INSERT(COALESCE(env, '{{}}'::VARIANT), '{}', '{}', true), \
             updated_at = NOW() WHERE agent_id = '{}'",
            sql::escape(key),
            sql::escape(value),
            sql::escape(agent_id)
        );
        let result = self.table.pool().exec(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "set_env_var",
                serde_json::json!({"agent_id": agent_id, "key": key}),
                error,
            );
        }
        result
    }

    pub async fn delete_env_var(&self, agent_id: &str, key: &str) -> Result<()> {
        let sql = format!(
            "UPDATE agent_config SET env = OBJECT_DELETE(env, '{}'), \
             updated_at = NOW() WHERE agent_id = '{}'",
            sql::escape(key),
            sql::escape(agent_id)
        );
        let result = self.table.pool().exec(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "delete_env_var",
                serde_json::json!({"agent_id": agent_id, "key": key}),
                error,
            );
        }
        result
    }

    pub async fn get_system_prompt(&self, agent_id: &str) -> Result<String> {
        let result = self
            .table
            .aggregate(
                "system_prompt",
                Some(&format!("agent_id = '{}'", sql::escape(agent_id))),
            )
            .await
            .map(|row| row.map(|r| sql::col(&r, 0)).unwrap_or_default());
        if let Err(error) = &result {
            repo_error(
                REPO,
                "get_system_prompt",
                serde_json::json!({"agent_id": agent_id}),
                error,
            );
        }
        result
    }
}

fn parse_optional_u64(raw: &str) -> Option<u64> {
    if raw.is_empty() || raw == "NULL" || raw == "null" {
        return None;
    }
    raw.parse::<u64>().ok()
}

fn parse_env_json(raw: &str) -> HashMap<String, String> {
    if raw.trim().is_empty() {
        return HashMap::new();
    }
    let val: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let obj = match val.as_object() {
        Some(o) => o,
        None => return HashMap::new(),
    };
    obj.iter()
        .map(|(k, v)| {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), s)
        })
        .collect()
}
