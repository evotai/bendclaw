use async_trait::async_trait;

use crate::kernel::channel::context::ChannelContext;
use crate::kernel::task::admin;
use crate::kernel::task::input::task_create_schema;
use crate::kernel::task::input::TaskCreateSpec;
use crate::kernel::task::view::TaskView;
use crate::kernel::tools::tool::OperationClassifier;
use crate::kernel::tools::tool::Tool;
use crate::kernel::tools::tool::ToolContext;
use crate::kernel::tools::tool::ToolResult;
use crate::kernel::tools::Impact;
use crate::kernel::tools::OpType;
use crate::kernel::tools::ToolId;
use crate::storage::dal::task::TaskDelivery;

pub struct TaskCreateTool {
    node_id: String,
}

impl TaskCreateTool {
    pub fn new(node_id: String) -> Self {
        Self { node_id }
    }
}

impl OperationClassifier for TaskCreateTool {
    fn op_type(&self) -> OpType {
        OpType::TaskWrite
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Medium)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("task")
            .to_string()
    }
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        ToolId::TaskCreate.as_str()
    }

    fn description(&self) -> &str {
        "Create a new scheduled task. Supports cron expressions, fixed intervals (every N seconds), or one-shot (at a specific time). When called from a channel (e.g. Feishu/Telegram), task results are automatically delivered back to the current chat."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        task_create_schema()
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> crate::base::Result<ToolResult> {
        let mut spec: TaskCreateSpec = match serde_json::from_value(args) {
            Ok(spec) => spec,
            Err(error) => {
                return Ok(ToolResult::error(format!(
                    "invalid task create input: {error}"
                )))
            }
        };

        // Auto-inject channel delivery from session context when not explicitly set
        if matches!(spec.delivery, TaskDelivery::None) {
            if let Some(ch) = ChannelContext::from_session_key(&ctx.session_id) {
                spec.delivery = TaskDelivery::Channel {
                    channel_account_id: ch.account_id,
                    chat_id: ch.chat_id,
                };
            }
        }

        let params = spec.into_params(
            self.node_id.clone(),
            ctx.user_id.to_string(),
            ctx.user_id.to_string(),
        );
        match admin::create_task(&ctx.pool, params).await {
            Ok(record) => Ok(ToolResult::ok(
                serde_json::to_string_pretty(&TaskView::from(record)).unwrap_or_default(),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to create task: {e}"))),
        }
    }
}
