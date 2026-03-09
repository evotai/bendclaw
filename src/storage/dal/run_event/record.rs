use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEventRecord {
    pub id: String,
    pub run_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub seq: u32,
    pub event: String,
    pub payload: String,
    pub created_at: String,
}
