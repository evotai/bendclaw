use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackRecord {
    pub id: String,
    pub agent_id: String,
    pub session_id: String,
    pub run_id: String,
    pub user_id: String,
    pub scope: String,
    pub created_by: String,
    pub rating: i32,
    pub comment: String,
    pub created_at: String,
    pub updated_at: String,
}
