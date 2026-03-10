use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub executor_instance_id: String,
    pub name: String,
    pub cron_expr: String,
    pub prompt: String,
    pub enabled: bool,
    pub status: String,
    pub schedule_kind: String,
    pub every_seconds: Option<i32>,
    pub at_time: Option<String>,
    pub tz: Option<String>,
    pub webhook_url: Option<String>,
    pub last_error: Option<String>,
    pub delete_after_run: bool,
    pub run_count: i32,
    pub last_run_at: String,
    pub next_run_at: Option<String>,
    pub lease_token: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
