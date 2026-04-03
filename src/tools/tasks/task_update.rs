use async_trait::async_trait;

use crate::storage::pool::Pool;
use crate::tasks::input::task_update_schema;
use crate::tasks::input::TaskUpdateToolInput;
use crate::tasks::ops;
use crate::tasks::view::TaskView;
use crate::tools::operation::Impact;
use crate::tools::operation::OpType;
use crate::tools::tool_context::ToolContext;
use crate::tools::tool_contract::OperationClassifier;
use crate::tools::tool_contract::Tool;
use crate::tools::tool_contract::ToolResult;
use crate::tools::tool_id::ToolId;

pub struct TaskUpdateTool {
    _node_id: String,
    pool: Pool,
}

impl TaskUpdateTool {
    pub fn new(node_id: String, pool: Pool) -> Self {
        Self {
            _node_id: node_id,
            pool,
        }
    }
}

impl OperationClassifier for TaskUpdateTool {
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
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        ToolId::TaskUpdate.as_str()
    }

    fn description(&self) -> &str {
        "Update an existing scheduled task. Only provided fields are changed."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        task_update_schema()
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> crate::types::Result<ToolResult> {
        let input: TaskUpdateToolInput = match serde_json::from_value(args) {
            Ok(input) => input,
            Err(error) => {
                return Ok(ToolResult::error(format!(
                    "invalid task update input: {error}"
                )))
            }
        };

        match ops::update_task(&self.pool, &input.task_id, input.spec.into_params()).await {
            Ok(updated) => Ok(ToolResult::ok(
                serde_json::to_string_pretty(&TaskView::from(updated)).unwrap_or_default(),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to update task: {e}"))),
        }
    }
}
