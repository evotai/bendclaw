use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableRecord {
    pub id: String,
    pub key: String,
    pub value: String,
    pub secret: bool,
    pub created_at: String,
    pub updated_at: String,
}
