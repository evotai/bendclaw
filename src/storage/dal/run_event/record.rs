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

impl RunEventRecord {
    pub fn payload_json(&self) -> crate::types::Result<serde_json::Value> {
        crate::storage::sql::parse_json(&self.payload, "run_events.payload")
    }
}
