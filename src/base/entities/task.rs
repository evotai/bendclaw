use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub name: String,
    pub prompt: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub schedule: serde_json::Value,
    #[serde(default)]
    pub delivery: serde_json::Value,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub delete_after_run: bool,
    #[serde(default)]
    pub run_count: i32,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_run_at: String,
    #[serde(default)]
    pub next_run_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn default_true() -> bool {
    true
}
