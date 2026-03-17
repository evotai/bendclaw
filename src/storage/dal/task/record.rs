use serde::Deserialize;
use serde::Serialize;

use super::delivery::TaskDelivery;
use super::schedule::TaskSchedule;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub executor_node_id: String,
    pub name: String,
    pub prompt: String,
    pub enabled: bool,
    pub status: String,
    pub schedule: TaskSchedule,
    #[serde(default)]
    pub delivery: TaskDelivery,
    pub last_error: Option<String>,
    pub delete_after_run: bool,
    pub run_count: i32,
    pub last_run_at: String,
    pub next_run_at: Option<String>,
    pub lease_token: Option<String>,
    pub lease_node_id: Option<String>,
    pub lease_expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
