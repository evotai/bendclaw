use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub skill_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub manifest: serde_json::Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn default_true() -> bool {
    true
}
