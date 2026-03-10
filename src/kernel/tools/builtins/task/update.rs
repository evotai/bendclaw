use async_trait::async_trait;
use serde_json::json;

use crate::kernel::task::admin;
use crate::kernel::task::admin::UpdateTaskParams;
use crate::kernel::tools::tool::OperationClassifier;
use crate::kernel::tools::tool::Tool;
use crate::kernel::tools::tool::ToolContext;
use crate::kernel::tools::tool::ToolResult;
use crate::kernel::tools::Impact;
use crate::kernel::tools::OpType;
use crate::kernel::tools::ToolId;
use crate::storage::TaskSchedule;

pub struct TaskUpdateTool {
    _instance_id: String,
}

impl TaskUpdateTool {
    pub fn new(instance_id: String) -> Self {
        Self {
            _instance_id: instance_id,
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
        json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "The task ID to update" },
                "name": { "type": "string", "description": "New task name" },
                "prompt": { "type": "string", "description": "New prompt" },
                "schedule": {
                    "type": "object",
                    "description": "New schedule configuration",
                    "properties": {
                        "kind": { "type": "string", "enum": ["cron", "every", "at"] },
                        "expr": { "type": "string", "description": "Cron expression (kind=cron)" },
                        "seconds": { "type": "integer", "description": "Interval in seconds (kind=every)" },
                        "time": { "type": "string", "description": "UTC timestamp (kind=at)" },
                        "tz": { "type": "string", "description": "Timezone" }
                    },
                    "required": ["kind"]
                },
                "webhook_url": { "type": "string", "description": "New webhook URL" },
                "delete_after_run": { "type": "boolean", "description": "Delete after first run" },
                "enabled": { "type": "boolean", "description": "Enable or disable the task" }
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

        let schedule = if let Some(sched) = args.get("schedule") {
            let kind = sched.get("kind").and_then(|v| v.as_str()).unwrap_or("cron");
            let s = match kind {
                "cron" => {
                    let expr = sched.get("expr").and_then(|v| v.as_str()).unwrap_or("");
                    let tz = sched
                        .get("tz")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    TaskSchedule::Cron {
                        expr: expr.to_string(),
                        tz,
                    }
                }
                "every" => {
                    let seconds =
                        sched.get("seconds").and_then(|v| v.as_i64()).unwrap_or(60) as i32;
                    TaskSchedule::Every { seconds }
                }
                "at" => {
                    let time = sched.get("time").and_then(|v| v.as_str()).unwrap_or("");
                    TaskSchedule::At {
                        time: time.to_string(),
                    }
                }
                _ => {
                    return Ok(ToolResult::error(
                        "schedule.kind must be one of: cron, every, at",
                    ))
                }
            };
            Some(s)
        } else {
            None
        };

        let webhook_url = args
            .get("webhook_url")
            .and_then(|v| v.as_str())
            .map(|s| Some(s.to_string()));

        let params = UpdateTaskParams {
            name: args
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            prompt: args
                .get("prompt")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            schedule,
            enabled: args.get("enabled").and_then(|v| v.as_bool()),
            webhook_url,
            delete_after_run: args.get("delete_after_run").and_then(|v| v.as_bool()),
        };

        match admin::update_task(&ctx.pool, task_id, params).await {
            Ok(updated) => Ok(ToolResult::ok(
                serde_json::to_string_pretty(&updated).unwrap_or_default(),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to update task: {e}"))),
        }
    }
}
