//! Task trial-run tool — manually trigger a task to execute immediately.

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
use crate::storage::dal::task::TaskRepo;
use crate::storage::pool::Pool;

pub struct TaskRunTool {
    pool: Pool,
}

impl TaskRunTool {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl OperationClassifier for TaskRunTool {
    fn op_type(&self) -> OpType {
        OpType::TaskWrite
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Medium)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let id = args
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        format!("trigger task {id}")
    }
}

#[async_trait]
impl Tool for TaskRunTool {
    fn name(&self) -> &str {
        ToolId::TaskRun.as_str()
    }

    fn description(&self) -> &str {
        "Trigger a scheduled task for immediate execution (trial run). The task will be picked up by the scheduler on the next cycle. Does not affect the regular schedule."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The ID of the task to trigger"
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
            Some(id) => id,
            None => return Ok(ToolResult::error("missing required parameter: task_id")),
        };

        let task = match management::get_task(&self.pool, task_id).await? {
            Some(t) => t,
            None => return Ok(ToolResult::error(format!("task {task_id} not found"))),
        };

        let repo = TaskRepo::new(self.pool.clone());
        if let Err(e) = repo.trigger_now(task_id).await {
            return Ok(ToolResult::error(format!("failed to trigger task: {e}")));
        }

        Ok(ToolResult::ok(
            json!({
                "task_id": task_id,
                "task_name": task.name,
                "status": "triggered",
                "message": "Task scheduled for immediate execution"
            })
            .to_string(),
        ))
    }
}
