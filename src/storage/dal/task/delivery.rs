use serde::Deserialize;
use serde::Serialize;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::storage::sql;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskDelivery {
    #[default]
    None,
    Webhook {
        url: String,
    },
    Channel {
        channel_account_id: String,
        chat_id: String,
    },
}

impl TaskDelivery {
    pub fn validate(&self) -> std::result::Result<(), String> {
        match self {
            TaskDelivery::None => Ok(()),
            TaskDelivery::Webhook { url } => {
                if url.trim().is_empty() {
                    return Err("delivery.webhook.url is required".to_string());
                }
                Ok(())
            }
            TaskDelivery::Channel {
                channel_account_id,
                chat_id,
            } => {
                if channel_account_id.trim().is_empty() {
                    return Err("delivery.channel.channel_account_id is required".to_string());
                }
                if chat_id.trim().is_empty() {
                    return Err("delivery.channel.chat_id is required".to_string());
                }
                Ok(())
            }
        }
    }

    pub fn from_storage(raw: &str, label: &str) -> Result<Self> {
        if raw.trim().is_empty() || raw.eq_ignore_ascii_case("null") {
            return Ok(Self::None);
        }
        sql::parse_json(raw, label)
    }

    pub fn to_storage_expr(&self) -> Result<String> {
        let json = serde_json::to_string(self)
            .map_err(|e| ErrorCode::storage_serde(format!("serialize task delivery: {e}")))?;
        Ok(format!("PARSE_JSON('{}')", sql::escape(&json)))
    }
}
