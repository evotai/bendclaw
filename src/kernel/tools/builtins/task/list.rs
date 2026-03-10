use async_trait::async_trait;
use serde_json::json;

use crate::kernel::task::admin;
use crate::kernel::tools::tool::OperationClassifier;
use crate::kernel::tools::tool::Tool;
use crate::kernel::tools::tool::ToolContext;
use crate::kernel::tools::tool::ToolResult;
use crate::kernel::tools::OpType;
use crate::kernel::tools::ToolId;

pub struct TaskListTool {
    _instance_id: String,
}

impl TaskListTool {
    pub fn new(instance_id: String) -> Self {
        Self {
            _instance_id: instance_id,
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
        "List scheduled tasks. Returns id, name, enabled, status, schedule_kind, and next_run_at for each task."
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
        ctx: &ToolContext,
    ) -> crate::base::Result<ToolResult> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as u32;

        match admin::list_tasks(&ctx.pool, limit).await {
            Ok(tasks) => {
                let items: Vec<serde_json::Value> = tasks
                    .iter()
                    .map(|t| {
                        json!({
                            "id": t.id,
                            "name": t.name,
                            "enabled": t.enabled,
                            "status": t.status,
                            "schedule_kind": t.schedule_kind,
                            "next_run_at": t.next_run_at,
                        })
                    })
                    .collect();
                Ok(ToolResult::ok(
                    serde_json::to_string_pretty(&items).unwrap_or_default(),
                ))
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to list tasks: {e}"))),
        }
    }
}
