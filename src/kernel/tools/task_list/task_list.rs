use async_trait::async_trait;
use serde_json::json;

use crate::kernel::task::admin;
use crate::kernel::task::view::TaskSummaryView;
use crate::kernel::tools::tool::OperationClassifier;
use crate::kernel::tools::tool::Tool;
use crate::kernel::tools::tool::ToolContext;
use crate::kernel::tools::tool::ToolResult;
use crate::kernel::tools::OpType;
use crate::kernel::tools::ToolId;
use crate::storage::pool::Pool;

pub struct TaskListTool {
    _node_id: String,
    pool: Pool,
}

impl TaskListTool {
    pub fn new(node_id: String, pool: Pool) -> Self {
        Self {
            _node_id: node_id,
            pool,
        }
    }
}

impl OperationClassifier for TaskListTool {
    fn op_type(&self) -> OpType {
        OpType::TaskRead
    }

    fn summarize(&self, _args: &serde_json::Value) -> String {
        "list tasks".into()
    }
}

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        ToolId::TaskList.as_str()
    }

    fn description(&self) -> &str {
        "List scheduled tasks. Returns id, name, enabled, status, schedule, and next_run_at for each task."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of tasks to return (default 20)",
                    "default": 20
                }
            },
            "required": []
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> crate::base::Result<ToolResult> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as u32;

        match admin::list_tasks(&self.pool, limit).await {
            Ok(tasks) => {
                let items: Vec<TaskSummaryView> =
                    tasks.into_iter().map(TaskSummaryView::from).collect();
                Ok(ToolResult::ok(
                    serde_json::to_string_pretty(&items).unwrap_or_default(),
                ))
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to list tasks: {e}"))),
        }
    }
}
