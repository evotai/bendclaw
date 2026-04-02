use async_trait::async_trait;
use serde_json::json;

use crate::kernel::task::management;
use crate::kernel::tools::operation::Impact;
use crate::kernel::tools::operation::OpType;
use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::storage::pool::Pool;

pub struct TaskDeleteTool {
    _node_id: String,
    pool: Pool,
}

impl TaskDeleteTool {
    pub fn new(node_id: String, pool: Pool) -> Self {
        Self {
            _node_id: node_id,
            pool,
        }
    }
}

impl OperationClassifier for TaskDeleteTool {
    fn op_type(&self) -> OpType {
        OpType::TaskWrite
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::High)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for TaskDeleteTool {
    fn name(&self) -> &str {
        ToolId::TaskDelete.as_str()
    }

    fn description(&self) -> &str {
        "Permanently delete a scheduled task by its ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID to delete"
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

        match management::delete_task(&self.pool, task_id).await {
            Ok(()) => Ok(ToolResult::ok(format!("Task '{task_id}' deleted"))),
            Err(e) => Ok(ToolResult::error(format!("Failed to delete task: {e}"))),
        }
    }
}
