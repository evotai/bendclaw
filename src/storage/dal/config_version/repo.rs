use super::record::ConfigVersionRecord;
use crate::llm::config::LLMConfig;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;
use crate::types::Result;

#[derive(Clone)]
struct VersionMapper;

impl RowMapper for VersionMapper {
    type Entity = ConfigVersionRecord;

    fn columns(&self) -> &str {
        "id, agent_id, version, label, `stage`, system_prompt, \
         identity, soul, token_limit_total, token_limit_daily, llm_config, notes, TO_VARCHAR(created_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::types::Result<ConfigVersionRecord> {
        Ok(ConfigVersionRecord {
            id: sql::col(row, 0),
            agent_id: sql::col(row, 1),
            version: sql::col_u32(row, 2)?,
            label: sql::col(row, 3),
            stage: sql::col(row, 4),
            system_prompt: sql::col(row, 5),
            identity: sql::col(row, 6),
            soul: sql::col(row, 7),
            token_limit_total: parse_optional_u64(&sql::col(row, 8)),
            token_limit_daily: parse_optional_u64(&sql::col(row, 9)),
            llm_config: parse_optional_json::<LLMConfig>(
                &sql::col(row, 10),
                "agent_config_versions.llm_config",
            )?,
            notes: sql::col(row, 11),
            created_at: sql::col(row, 12),
        })
    }
}

#[derive(Clone)]
pub struct ConfigVersionRepo {
    table: DatabendTable<VersionMapper>,
}

impl ConfigVersionRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "agent_config_versions", VersionMapper),
        }
    }

    pub async fn insert(&self, record: &ConfigVersionRecord) -> Result<()> {
        let total_str = match record.token_limit_total {
            Some(v) => format!("{v}"),
            None => "NULL".to_string(),
        };
        let daily_str = match record.token_limit_daily {
            Some(v) => format!("{v}"),
            None => "NULL".to_string(),
        };
        let llm_expr = match &record.llm_config {
            Some(cfg) => {
                let json = serde_json::to_string(cfg).unwrap_or_else(|_| "null".to_string());
                format!("PARSE_JSON('{}')", sql::escape(&json))
            }
            None => "NULL".to_string(),
        };
        self.table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("agent_id", SqlVal::Str(&record.agent_id)),
                ("version", SqlVal::Raw(&record.version.to_string())),
                ("label", SqlVal::Str(&record.label)),
                ("`stage`", SqlVal::Str(&record.stage)),
                ("system_prompt", SqlVal::Str(&record.system_prompt)),
                ("identity", SqlVal::Str(&record.identity)),
                ("soul", SqlVal::Str(&record.soul)),
                ("token_limit_total", SqlVal::Raw(&total_str)),
                ("token_limit_daily", SqlVal::Raw(&daily_str)),
                ("llm_config", SqlVal::Raw(&llm_expr)),
                ("notes", SqlVal::Str(&record.notes)),
                ("created_at", SqlVal::Raw("NOW()")),
            ])
            .await
    }

    pub async fn list_by_agent(
        &self,
        agent_id: &str,
        limit: u32,
    ) -> Result<Vec<ConfigVersionRecord>> {
        self.table
            .list(
                &[Where("agent_id", SqlVal::Str(agent_id))],
                "version DESC",
                limit as u64,
            )
            .await
    }

    pub async fn get_version(
        &self,
        agent_id: &str,
        version: u32,
    ) -> Result<Option<ConfigVersionRecord>> {
        let aid = sql::escape(agent_id);
        let cond = format!("agent_id = '{aid}' AND version = {version}");
        let rows = self.table.list_where(&cond, "version DESC", 1).await?;
        Ok(rows.into_iter().next())
    }

    pub async fn next_version(&self, agent_id: &str) -> Result<u32> {
        let aid = sql::escape(agent_id);
        let q = format!(
            "SELECT COALESCE(MAX(version), 0) FROM agent_config_versions WHERE agent_id = '{aid}'"
        );
        let row = self.table.pool().query_row(&q).await?;
        let max = sql::agg_u64_or_zero(row.as_ref(), 0)? as u32;
        Ok(max + 1)
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
