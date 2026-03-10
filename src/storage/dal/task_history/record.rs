use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryRecord {
    pub id: String,
    pub task_id: String,
    pub run_id: Option<String>,
    pub task_name: String,
    pub schedule_kind: String,
    pub cron_expr: Option<String>,
    pub prompt: String,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<i32>,
    pub webhook_url: Option<String>,
    pub webhook_status: Option<String>,
    pub webhook_error: Option<String>,
    pub executed_by_instance_id: Option<String>,
    pub created_at: String,
}
