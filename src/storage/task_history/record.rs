use serde::Deserialize;
use serde::Serialize;

use crate::storage::dal::task::TaskDelivery;
use crate::storage::dal::task::TaskSchedule;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryRecord {
    pub id: String,
    pub task_id: String,
    pub run_id: Option<String>,
    pub task_name: String,
    pub schedule: TaskSchedule,
    pub prompt: String,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<i32>,
    #[serde(default)]
    pub delivery: TaskDelivery,
    pub delivery_status: Option<String>,
    pub delivery_error: Option<String>,
    pub user_id: String,
    pub executed_by_node_id: Option<String>,
    pub created_at: String,
}
