use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistory {
    pub history_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub user_id: String,
    #[serde(default)]
    pub run_id: Option<String>,
    pub task_name: String,
    #[serde(default)]
    pub schedule: serde_json::Value,
    pub prompt: String,
    pub status: String,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i32>,
    #[serde(default)]
    pub delivery: serde_json::Value,
    #[serde(default)]
    pub delivery_status: Option<String>,
    #[serde(default)]
    pub delivery_error: Option<String>,
    #[serde(default)]
    pub executed_by_node_id: Option<String>,
    pub created_at: String,
}
