use super::record::ConfigVersionRecord;
use crate::base::Result;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

#[derive(Clone)]
struct VersionMapper;

impl RowMapper for VersionMapper {
    type Entity = ConfigVersionRecord;

    fn columns(&self) -> &str {
        "id, agent_id, version, label, `stage`, system_prompt, display_name, description, \
         identity, soul, token_limit_total, token_limit_daily, notes, TO_VARCHAR(created_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> ConfigVersionRecord {
        ConfigVersionRecord {
            id: sql::col(row, 0),
            agent_id: sql::col(row, 1),
            version: sql::col(row, 2).parse().unwrap_or(0),
            label: sql::col(row, 3),
            stage: sql::col(row, 4),
            system_prompt: sql::col(row, 5),
            display_name: sql::col(row, 6),
            description: sql::col(row, 7),
            identity: sql::col(row, 8),
            soul: sql::col(row, 9),
            token_limit_total: parse_optional_u64(&sql::col(row, 10)),
            token_limit_daily: parse_optional_u64(&sql::col(row, 11)),
            notes: sql::col(row, 12),
            created_at: sql::col(row, 13),
        }
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
        self.table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("agent_id", SqlVal::Str(&record.agent_id)),
                ("version", SqlVal::Raw(&record.version.to_string())),
                ("label", SqlVal::Str(&record.label)),
                ("`stage`", SqlVal::Str(&record.stage)),
                ("system_prompt", SqlVal::Str(&record.system_prompt)),
                ("display_name", SqlVal::Str(&record.display_name)),
                ("description", SqlVal::Str(&record.description)),
                ("identity", SqlVal::Str(&record.identity)),
                ("soul", SqlVal::Str(&record.soul)),
                ("token_limit_total", SqlVal::Raw(&total_str)),
                ("token_limit_daily", SqlVal::Raw(&daily_str)),
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
        let max: u32 = row
            .and_then(|r| {
                r.as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0);
        Ok(max + 1)
    }
}

fn parse_optional_u64(raw: &str) -> Option<u64> {
    if raw.is_empty() || raw == "NULL" || raw == "null" {
        return None;
    }
    raw.parse::<u64>().ok()
}
