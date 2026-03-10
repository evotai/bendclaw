use async_trait::async_trait;
use serde_json::json;

use crate::kernel::task::admin;
use crate::kernel::task::admin::CreateTaskParams;
use crate::kernel::tools::tool::OperationClassifier;
use crate::kernel::tools::tool::Tool;
use crate::kernel::tools::tool::ToolContext;
use crate::kernel::tools::tool::ToolResult;
use crate::kernel::tools::Impact;
use crate::kernel::tools::OpType;
use crate::kernel::tools::ToolId;
use crate::storage::TaskSchedule;

pub struct TaskCreateTool {
    instance_id: String,
}

impl TaskCreateTool {
    pub fn new(instance_id: String) -> Self {
        Self { instance_id }
    }
}

impl OperationClassifier for TaskCreateTool {
    fn op_type(&self) -> OpType {
        OpType::TaskWrite
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Medium)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("task")
            .to_string()
    }
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        ToolId::TaskCreate.as_str()
    }

    fn description(&self) -> &str {
        "Create a new scheduled task. Supports cron expressions, fixed intervals (every N seconds), or one-shot (at a specific time)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Human-readable task name"
                },
                "prompt": {
                    "type": "string",
                    "description": "The prompt to execute when the task runs"
                },
                "schedule": {
                    "type": "object",
                    "description": "Schedule configuration",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": ["cron", "every", "at"],
                            "description": "Schedule type"
                        },
                        "expr": { "type": "string", "description": "Cron expression (kind=cron)" },
                        "seconds": { "type": "integer", "description": "Interval in seconds (kind=every)" },
                        "time": { "type": "string", "description": "UTC timestamp (kind=at)" },
                        "tz": { "type": "string", "description": "Timezone (e.g. 'Asia/Shanghai')" }
                    },
                    "required": ["kind"]
                },
                "webhook_url": { "type": "string", "description": "Optional webhook URL to call after execution" },
                "delete_after_run": { "type": "boolean", "description": "Delete after first run (default false)", "default": false }
            },
            "required": ["name", "prompt", "schedule"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> crate::base::Result<ToolResult> {
        let name = match args.get("name").and_then(|v| v.as_str()) {
            Some(n) if !n.is_empty() => n,
            _ => return Ok(ToolResult::error("name is required")),
        };
        let prompt = match args.get("prompt").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(ToolResult::error("prompt is required")),
        };
        let sched = match args.get("schedule") {
            Some(s) => s,
            None => return Ok(ToolResult::error("schedule is required")),
        };
        let kind = sched.get("kind").and_then(|v| v.as_str()).unwrap_or("cron");
        let schedule = match kind {
            "cron" => {
                let expr = sched.get("expr").and_then(|v| v.as_str()).unwrap_or("");
                if expr.is_empty() {
                    return Ok(ToolResult::error("schedule.expr is required for cron"));
                }
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
                let seconds = match sched.get("seconds").and_then(|v| v.as_i64()) {
                    Some(s) => s as i32,
                    None => return Ok(ToolResult::error("schedule.seconds is required for every")),
                };
                TaskSchedule::Every { seconds }
            }
            "at" => {
                let time = match sched.get("time").and_then(|v| v.as_str()) {
                    Some(t) if !t.is_empty() => t,
                    _ => return Ok(ToolResult::error("schedule.time is required for at")),
                };
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

        let webhook_url = args.get("webhook_url").and_then(|v| v.as_str());
        let delete_after_run = args
            .get("delete_after_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let params = CreateTaskParams {
            executor_instance_id: self.instance_id.clone(),
            name: name.to_string(),
            prompt: prompt.to_string(),
            schedule,
            webhook_url: webhook_url.map(|s| s.to_string()),
            delete_after_run,
        };

        match admin::create_task(&ctx.pool, params).await {
            Ok(record) => Ok(ToolResult::ok(
                serde_json::to_string_pretty(&json!({
                    "id": record.id,
                    "name": record.name,
                    "schedule_kind": record.schedule_kind,
                    "next_run_at": record.next_run_at,
                    "enabled": record.enabled,
                }))
                .unwrap_or_default(),
            )),
            Err(e) => Ok(ToolResult::error(format!("Failed to create task: {e}"))),
        }
    }
}
