use super::record::ChannelAccountRecord;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;
use crate::types::Result;

#[derive(Clone)]
struct Mapper;

impl RowMapper for Mapper {
    type Entity = ChannelAccountRecord;

    fn columns(&self) -> &str {
        "id, channel_type, account_id, agent_id, user_id, scope, node_id, created_by, config, enabled, lease_node_id, lease_token, TO_VARCHAR(lease_expires_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::types::Result<Self::Entity> {
        let config_raw: String = sql::col(row, 8);
        let config: serde_json::Value = sql::parse_json(&config_raw, "channel_accounts.config")?;
        Ok(ChannelAccountRecord {
            id: sql::col(row, 0),
            channel_type: sql::col(row, 1),
            account_id: sql::col(row, 2),
            agent_id: sql::col(row, 3),
            user_id: sql::col(row, 4),
            scope: sql::col(row, 5),
            node_id: sql::col(row, 6),
            created_by: sql::col(row, 7),
            config,
            enabled: sql::col(row, 9) == "1",
            lease_node_id: sql::col_opt(row, 10),
            lease_token: sql::col_opt(row, 11),
            lease_expires_at: sql::col_opt(row, 12),
            created_at: sql::col(row, 13),
            updated_at: sql::col(row, 14),
        })
    }
}

#[derive(Clone)]
pub struct ChannelAccountRepo {
    table: DatabendTable<Mapper>,
}

impl ChannelAccountRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "channel_accounts", Mapper),
        }
    }

    pub async fn insert(&self, record: &ChannelAccountRecord) -> Result<()> {
        let enabled_str = if record.enabled { "1" } else { "0" };
        let config_str = serde_json::to_string(&record.config)?;
        self.table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("channel_type", SqlVal::Str(&record.channel_type)),
                ("account_id", SqlVal::Str(&record.account_id)),
                ("agent_id", SqlVal::Str(&record.agent_id)),
                ("user_id", SqlVal::Str(&record.user_id)),
                ("scope", SqlVal::Str(&record.scope)),
                ("node_id", SqlVal::Str(&record.node_id)),
                ("created_by", SqlVal::Str(&record.created_by)),
                (
                    "config",
                    SqlVal::Raw(&format!(
                        "PARSE_JSON('{}')",
                        crate::storage::sql::escape(&config_str)
                    )),
                ),
                ("enabled", SqlVal::Raw(enabled_str)),
                ("created_at", SqlVal::Raw("NOW()")),
                ("updated_at", SqlVal::Raw("NOW()")),
            ])
            .await
    }

    pub async fn load(&self, id: &str) -> Result<Option<ChannelAccountRecord>> {
        self.table.get(&[Where("id", SqlVal::Str(id))]).await
    }

    pub async fn list_by_agent(&self, agent_id: &str) -> Result<Vec<ChannelAccountRecord>> {
        self.table
            .list(
                &[Where("agent_id", SqlVal::Str(agent_id))],
                "created_at DESC",
                1000,
            )
            .await
    }

    pub async fn list_by_type(&self, channel_type: &str) -> Result<Vec<ChannelAccountRecord>> {
        self.table
            .list(
                &[Where("channel_type", SqlVal::Str(channel_type))],
                "created_at DESC",
                1000,
            )
            .await
    }

    pub async fn update_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let val = if enabled { "1" } else { "0" };
        let sql = format!(
            "UPDATE channel_accounts SET enabled = {val}, updated_at = NOW() WHERE id = '{}'",
            crate::storage::sql::escape(id)
        );
        self.table.pool().exec(&sql).await
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        self.table.delete(&[Where("id", SqlVal::Str(id))]).await
    }

    /// Release the receiver lease for a channel account (used on delete or disable).
    pub async fn release_lease(&self, id: &str) -> Result<()> {
        let sql_str = sql::Sql::update("channel_accounts")
            .set_raw("lease_node_id", "NULL")
            .set_raw("lease_token", "NULL")
            .set_raw("lease_expires_at", "NULL")
            .set_raw("updated_at", "NOW()")
            .where_eq("id", id)
            .build();
        self.table.pool().exec(&sql_str).await
    }
}
