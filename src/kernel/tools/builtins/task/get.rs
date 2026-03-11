use async_trait::async_trait;
use serde_json::json;

use crate::kernel::task::admin;
use crate::kernel::task::view::TaskView;
use crate::kernel::tools::tool::OperationClassifier;
use crate::kernel::tools::tool::Tool;
use crate::kernel::tools::tool::ToolContext;
use crate::kernel::tools::tool::ToolResult;
use crate::kernel::tools::OpType;
use crate::kernel::tools::ToolId;

pub struct TaskGetTool {
    _instance_id: String,
}

impl TaskGetTool {
    pub fn new(instance_id: String) -> Self {
        Self {
            _instance_id: instance_id,
        }
    }
}

impl OperationClassifier for TaskGetTool {
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
impl Tool for TaskGetTool {
    fn name(&self) -> &str {
        ToolId::TaskGet.as_str()
    }

    fn description(&self) -> &str {
        "Get full details of a scheduled task by its ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID to retrieve"
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

        match admin::get_task(&ctx.pool, task_id).await {
            Ok(Some(task)) => Ok(ToolResult::ok(
                serde_json::to_string_pretty(&TaskView::from(task)).unwrap_or_default(),
            )),
            Ok(None) => Ok(ToolResult::error(format!("Task '{task_id}' not found"))),
            Err(e) => Ok(ToolResult::error(format!("Failed to get task: {e}"))),
        }
    }
}
