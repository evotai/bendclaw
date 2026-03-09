use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningRecord {
    pub id: String,
    pub agent_id: String,
    pub user_id: String,
    pub session_id: String,
    pub title: String,
    pub content: String,
    pub tags: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}
