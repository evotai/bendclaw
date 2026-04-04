use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    pub title: Option<String>,
    pub turns: u32,
    pub created_at: String,
    pub updated_at: String,
}

impl SessionMeta {
    pub fn new(session_id: String, cwd: String, model: String) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            session_id,
            cwd,
            model,
            title: None,
            turns: 0,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

pub struct SessionState {
    pub meta: SessionMeta,
    pub messages: Vec<bend_agent::Message>,
}

impl SessionState {
    pub fn new(meta: SessionMeta, messages: Vec<bend_agent::Message>) -> Self {
        Self { meta, messages }
    }
}
