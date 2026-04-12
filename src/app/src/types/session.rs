//! Session metadata types.

use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

// ---------------------------------------------------------------------------
// SessionMeta — session metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    pub title: Option<String>,
    pub turns: u32,
    /// Number of context messages at last save.
    #[serde(default)]
    pub message_count: u32,
    /// Estimated context tokens at last save.
    #[serde(default)]
    pub context_tokens: usize,
    /// Context budget (window − system prompt) at last save.
    #[serde(default)]
    pub context_budget: usize,
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
            message_count: 0,
            context_tokens: 0,
            context_budget: 0,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

// ---------------------------------------------------------------------------
// ListSessions — query for listing sessions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListSessions {
    pub limit: usize,
}
