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

pub struct TaskToggleTool {
    _instance_id: String,
}

impl TaskToggleTool {
    pub fn new(instance_id: String) -> Self {
        Self {
            _instance_id: instance_id,
        }
    }
}

impl OperationClassifier for TaskToggleTool {
    fn op_type(&self) -> OpType {
        OpType::TaskWrite
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Medium)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for TaskToggleTool {
    fn name(&self) -> &str {
        ToolId::TaskToggle.as_str()
    }

    fn description(&self) -> &str {
        "Toggle a task's enabled state (enable if disabled, disable if enabled). Returns the updated task."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID to toggle"
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

        match admin::toggle_task(&ctx.pool, task_id).await {
            Ok(task) => Ok(ToolResult::ok(
                serde_json::to_string_pretty(&json!({
                    "id": task.id,
                    "name": task.name,
                    "enabled": task.enabled,
                    "status": task.status,
                }))
                .unwrap_or_default(),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to toggle task: {e}"))),
        }
    }
}
