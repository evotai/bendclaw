use async_trait::async_trait;
use serde_json::json;

use crate::kernel::task::admin;
use crate::kernel::task::view::TaskHistoryView;
use crate::kernel::tools::tool::OperationClassifier;
use crate::kernel::tools::tool::Tool;
use crate::kernel::tools::tool::ToolContext;
use crate::kernel::tools::tool::ToolResult;
use crate::kernel::tools::OpType;
use crate::kernel::tools::ToolId;
use crate::storage::pool::Pool;

pub struct TaskHistoryTool {
    _node_id: String,
    pool: Pool,
}

impl TaskHistoryTool {
    pub fn new(node_id: String, pool: Pool) -> Self {
        Self {
            _node_id: node_id,
            pool,
        }
    }
}

impl OperationClassifier for TaskHistoryTool {
    fn op_type(&self) -> OpType {
        OpType::TaskRead
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for TaskHistoryTool {
    fn name(&self) -> &str {
        ToolId::TaskHistory.as_str()
    }

    fn description(&self) -> &str {
        "List execution history for a task. Returns recent runs with status, output, errors, and duration."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID to get history for"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of history entries to return (default 10)",
                    "default": 10
                }
            },
            "required": ["task_id"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> crate::base::Result<ToolResult> {
        let task_id = match args.get("task_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id,
            _ => return Ok(ToolResult::error("task_id is required")),
        };

        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as u32;

        match admin::list_task_history(&self.pool, task_id, limit).await {
            Ok(entries) => Ok(ToolResult::ok(
                serde_json::to_string_pretty(
                    &entries
                        .into_iter()
                        .map(TaskHistoryView::from)
                        .collect::<Vec<_>>(),
                )
                .unwrap_or_default(),
            )),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to get task history: {e}"
            ))),
        }
    }
}
