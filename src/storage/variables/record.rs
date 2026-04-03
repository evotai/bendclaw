use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableRecord {
    pub id: String,
    pub key: String,
    pub value: String,
    pub secret: bool,
    pub revoked: bool,
    pub user_id: String,
    pub scope: String,
    pub created_by: String,
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
