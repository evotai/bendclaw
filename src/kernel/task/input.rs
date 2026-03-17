use serde::Deserialize;
use serde_json::json;

use super::admin::CreateTaskParams;
use super::admin::UpdateTaskParams;
use crate::storage::TaskDelivery;
use crate::storage::TaskSchedule;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCreateSpec {
    pub name: String,
    pub prompt: String,
    pub schedule: TaskSchedule,
    #[serde(default)]
    pub delivery: TaskDelivery,
    #[serde(default)]
    pub delete_after_run: bool,
}

impl TaskCreateSpec {
    pub fn into_params(self, executor_node_id: String) -> CreateTaskParams {
        CreateTaskParams {
            executor_node_id,
            name: self.name,
            prompt: self.prompt,
            schedule: self.schedule,
            delivery: self.delivery,
            delete_after_run: self.delete_after_run,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskUpdateSpec {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub schedule: Option<TaskSchedule>,
    pub enabled: Option<bool>,
    pub delivery: Option<TaskDelivery>,
    pub delete_after_run: Option<bool>,
}

impl TaskUpdateSpec {
    pub fn into_params(self) -> UpdateTaskParams {
        UpdateTaskParams {
            name: self.name,
            prompt: self.prompt,
            schedule: self.schedule,
            enabled: self.enabled,
            delivery: self.delivery,
            delete_after_run: self.delete_after_run,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskUpdateToolInput {
    pub task_id: String,
    #[serde(flatten)]
    pub spec: TaskUpdateSpec,
}

pub fn task_schedule_schema() -> serde_json::Value {
    json!({
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
    })
}

pub fn task_delivery_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "description": "Optional delivery target for task results",
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["none", "webhook", "channel"],
                "description": "Delivery type"
            },
            "url": {
                "type": "string",
                "description": "Webhook URL (kind=webhook)"
            },
            "channel_account_id": {
                "type": "string",
                "description": "Channel account ID (kind=channel)"
            },
            "chat_id": {
                "type": "string",
                "description": "Target chat or conversation ID (kind=channel)"
            }
        },
        "required": ["kind"]
    })
}

pub fn task_create_schema() -> serde_json::Value {
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
            "schedule": task_schedule_schema(),
            "delivery": task_delivery_schema(),
            "delete_after_run": {
                "type": "boolean",
                "description": "Delete after first run (default false)",
                "default": false
            }
        },
        "required": ["name", "prompt", "schedule"]
    })
}

pub fn task_update_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "task_id": { "type": "string", "description": "The task ID to update" },
            "name": { "type": "string", "description": "New task name" },
            "prompt": { "type": "string", "description": "New prompt" },
            "schedule": task_schedule_schema(),
            "delivery": task_delivery_schema(),
            "delete_after_run": {
                "type": "boolean",
                "description": "Delete after first run"
            },
            "enabled": {
                "type": "boolean",
                "description": "Enable or disable the task"
            }
        },
        "required": ["task_id"]
    })
}
