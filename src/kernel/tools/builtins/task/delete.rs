use async_trait::async_trait;
use serde_json::json;

use crate::kernel::task::admin;
use crate::kernel::tools::tool::OperationClassifier;
use crate::kernel::tools::tool::Tool;
use crate::kernel::tools::tool::ToolContext;
use crate::kernel::tools::tool::ToolResult;
use crate::kernel::tools::Impact;
use crate::kernel::tools::OpType;
use crate::kernel::tools::ToolId;

pub struct TaskDeleteTool {
    _instance_id: String,
}

impl TaskDeleteTool {
    pub fn new(instance_id: String) -> Self {
        Self {
            _instance_id: instance_id,
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
        ctx: &ToolContext,
    ) -> crate::base::Result<ToolResult> {
        let task_id = match args.get("task_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id,
            _ => return Ok(ToolResult::error("task_id is required")),
        };

        match admin::delete_task(&ctx.pool, task_id).await {
            Ok(()) => Ok(ToolResult::ok(format!("Task '{task_id}' deleted"))),
            Err(e) => Ok(ToolResult::error(format!("Failed to delete task: {e}"))),
        }
    }
}
