use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::channels::runtime::channel_registry::ChannelRegistry;
use crate::channels::send_text_to_account;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;
use crate::storage::pool::Pool;
use crate::tools::tool_context::ToolContext;
use crate::tools::tool_contract::OperationClassifier;
use crate::tools::tool_contract::Tool;
use crate::tools::tool_contract::ToolResult;
use crate::tools::tool_id::ToolId;
use crate::tools::Impact;
use crate::tools::OpType;
use crate::types::Result;

/// Send a message to an external channel (Telegram, Feishu, GitHub, etc.).
pub struct ChannelSendTool {
    channels: Arc<ChannelRegistry>,
    pool: Pool,
}

impl ChannelSendTool {
    pub fn new(channels: Arc<ChannelRegistry>, pool: Pool) -> Self {
        Self { channels, pool }
    }
}

impl OperationClassifier for ChannelSendTool {
    fn op_type(&self) -> OpType {
        OpType::Execute
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Medium)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let channel_type = args
            .get("channel_type")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let chat_id = args.get("chat_id").and_then(|v| v.as_str()).unwrap_or("?");
        format!("send to {channel_type}:{chat_id}")
    }
}

#[async_trait]
impl Tool for ChannelSendTool {
    fn name(&self) -> &str {
        ToolId::ChannelSend.as_str()
    }

    fn description(&self) -> &str {
        "Send a message to an external channel. Requires a channel account to be configured for the agent."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "channel_type": {
                    "type": "string",
                    "description": "Channel type (e.g. 'telegram', 'feishu', 'github')"
                },
                "chat_id": {
                    "type": "string",
                    "description": "Target chat/conversation/issue ID on the platform"
                },
                "text": {
                    "type": "string",
                    "description": "Message text to send"
                }
            },
            "required": ["channel_type", "chat_id", "text"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let channel_type = match args.get("channel_type").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => return Ok(ToolResult::error("Missing 'channel_type' parameter")),
        };
        let chat_id = match args.get("chat_id").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => return Ok(ToolResult::error("Missing 'chat_id' parameter")),
        };
        let text = match args.get("text").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => return Ok(ToolResult::error("Missing 'text' parameter")),
        };

        if self.channels.get(channel_type).is_none() {
            return Ok(ToolResult::error(format!(
                "Unknown channel type: {channel_type}"
            )));
        }

        // Find the first enabled account of this channel type for the agent.
        let repo = ChannelAccountRepo::new(self.pool.clone());
        let accounts = match repo.list_by_agent(&ctx.agent_id).await {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::error(format!("Failed to list accounts: {e}"))),
        };
        let account = match accounts
            .iter()
            .find(|a| a.channel_type == channel_type && a.enabled)
        {
            Some(a) => a,
            None => {
                return Ok(ToolResult::error(format!(
                    "No enabled {channel_type} account configured for this agent"
                )))
            }
        };

        match send_text_to_account(self.channels.as_ref(), account, chat_id, text).await {
            Ok(msg_id) => Ok(ToolResult::ok(format!("Message sent (id: {msg_id})"))),
            Err(e) => Ok(ToolResult::error(format!("Failed to send: {e}"))),
        }
    }
}
