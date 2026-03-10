use crate::base::Result;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

use super::record::ChannelMessageRecord;

#[derive(Clone)]
struct Mapper;

impl RowMapper for Mapper {
    type Entity = ChannelMessageRecord;

    fn columns(&self) -> &str {
        "id, channel_type, account_id, chat_id, session_id, direction, sender_id, text, platform_message_id, run_id, attachments, TO_VARCHAR(created_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> Self::Entity {
        ChannelMessageRecord {
            id: sql::col(row, 0),
            channel_type: sql::col(row, 1),
            account_id: sql::col(row, 2),
            chat_id: sql::col(row, 3),
            session_id: sql::col(row, 4),
            direction: sql::col(row, 5),
            sender_id: sql::col(row, 6),
            text: sql::col(row, 7),
            platform_message_id: sql::col(row, 8),
            run_id: sql::col(row, 9),
            attachments: sql::col(row, 10),
            created_at: sql::col(row, 11),
        }
    }
}

#[derive(Clone)]
pub struct ChannelMessageRepo {
    table: DatabendTable<Mapper>,
}

impl ChannelMessageRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "channel_messages", Mapper),
        }
    }

    pub async fn insert(&self, record: &ChannelMessageRecord) -> Result<()> {
        self.table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("channel_type", SqlVal::Str(&record.channel_type)),
                ("account_id", SqlVal::Str(&record.account_id)),
                ("chat_id", SqlVal::Str(&record.chat_id)),
                ("session_id", SqlVal::Str(&record.session_id)),
                ("direction", SqlVal::Str(&record.direction)),
                ("sender_id", SqlVal::Str(&record.sender_id)),
                ("text", SqlVal::Str(&record.text)),
                ("platform_message_id", SqlVal::Str(&record.platform_message_id)),
                ("run_id", SqlVal::Str(&record.run_id)),
                ("attachments", SqlVal::Str(&record.attachments)),
                ("created_at", SqlVal::Raw("NOW()")),
            ])
            .await
    }

    pub async fn list_by_chat(
        &self,
        channel_type: &str,
        chat_id: &str,
        limit: u64,
    ) -> Result<Vec<ChannelMessageRecord>> {
        self.table
            .list(
                &[
                    Where("channel_type", SqlVal::Str(channel_type)),
                    Where("chat_id", SqlVal::Str(chat_id)),
                ],
                "created_at DESC",
                limit,
            )
            .await
    }

    pub async fn list_by_session(
        &self,
        session_id: &str,
        limit: u64,
    ) -> Result<Vec<ChannelMessageRecord>> {
        self.table
            .list(
                &[Where("session_id", SqlVal::Str(session_id))],
                "created_at DESC",
                limit,
            )
            .await
    }
}
