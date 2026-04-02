use async_trait::async_trait;
use serde_json::json;

use crate::kernel::task::management;
use crate::kernel::task::view::TaskView;
use crate::kernel::tools::operation::Impact;
use crate::kernel::tools::operation::OpType;
use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::storage::pool::Pool;

pub struct TaskToggleTool {
    _node_id: String,
    pool: Pool,
}

impl TaskToggleTool {
    pub fn new(node_id: String, pool: Pool) -> Self {
        Self {
            _node_id: node_id,
            pool,
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
        _ctx: &ToolContext,
    ) -> crate::base::Result<ToolResult> {
        let task_id = match args.get("task_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id,
            _ => return Ok(ToolResult::error("task_id is required")),
        };

        match management::toggle_task(&self.pool, task_id).await {
            Ok(task) => Ok(ToolResult::ok(
                serde_json::to_string_pretty(&TaskView::from(task)).unwrap_or_default(),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to toggle task: {e}"))),
        }
    }
}
