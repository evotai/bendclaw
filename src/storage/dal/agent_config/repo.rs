use std::time::Duration;

use super::record::AgentConfigRecord;
use crate::llm::config::LLMConfig;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;
use crate::types::Result;

const REPO: &str = "agent_config";
const CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct ConfigMapper;

impl RowMapper for ConfigMapper {
    type Entity = AgentConfigRecord;

    fn columns(&self) -> &str {
        "agent_id, system_prompt, \
         identity, soul, token_limit_total, token_limit_daily, \
         llm_config, TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::types::Result<AgentConfigRecord> {
        Ok(AgentConfigRecord {
            agent_id: sql::col(row, 0),
            system_prompt: sql::col(row, 1),
            identity: sql::col(row, 2),
            soul: sql::col(row, 3),
            token_limit_total: parse_optional_u64(&sql::col(row, 4)),
            token_limit_daily: parse_optional_u64(&sql::col(row, 5)),
            llm_config: parse_optional_json::<LLMConfig>(
                &sql::col(row, 6),
                "agent_config.llm_config",
            )?,
            updated_at: sql::col(row, 7),
        })
    }
}

#[derive(Clone)]
pub struct AgentConfigStore {
    table: DatabendTable<ConfigMapper>,
}

impl AgentConfigStore {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "agent_config", ConfigMapper).with_ttl_cache(CACHE_TTL),
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
        identity: Option<&str>,
        soul: Option<&str>,
        token_limit_total: Option<Option<u64>>,
        token_limit_daily: Option<Option<u64>>,
        llm_config: Option<&str>,
    ) -> Result<()> {
        let total_str = match token_limit_total {
            Some(Some(v)) => format!("{v}"),
            _ => "NULL".to_string(),
        };
        let daily_str = match token_limit_daily {
            Some(Some(v)) => format!("{v}"),
            _ => "NULL".to_string(),
        };
        let llm_expr = match llm_config {
            Some(json) => format!("PARSE_JSON('{}')", sql::escape(json)),
            None => "NULL".to_string(),
        };

        let result = self
            .table
            .upsert(
                &[
                    ("agent_id", SqlVal::Str(agent_id)),
                    ("system_prompt", SqlVal::Str(system_prompt.unwrap_or(""))),
                    ("identity", SqlVal::Str(identity.unwrap_or(""))),
                    ("soul", SqlVal::Str(soul.unwrap_or(""))),
                    ("token_limit_total", SqlVal::Raw(&total_str)),
                    ("token_limit_daily", SqlVal::Raw(&daily_str)),
                    ("llm_config", SqlVal::Raw(&llm_expr)),
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

fn parse_optional_json<T: serde::de::DeserializeOwned>(
    raw: &str,
    label: &str,
) -> crate::types::Result<Option<T>> {
    if raw.is_empty() || raw == "NULL" || raw == "null" {
        return Ok(None);
    }
    sql::parse_json(raw, label).map(Some)
}
