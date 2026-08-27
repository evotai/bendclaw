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
    /// Provider paired with `model`. Empty for sessions persisted before this
    /// field existed; resume callers then fall back to legacy model-only lookup.
    #[serde(default)]
    pub provider: String,
    /// Historical reasoning effort used by this session, as a lowercase level
    /// name (e.g. `"high"`). Retained for metadata compatibility and diagnostics;
    /// resume reapplies the current provider configuration instead.
    #[serde(default)]
    pub thinking_level: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub source: String,
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
    /// Cumulative input tokens billed across all runs in this session.
    #[serde(default)]
    pub total_input_tokens: u64,
    /// Cumulative output tokens billed across all runs in this session.
    #[serde(default)]
    pub total_output_tokens: u64,
    /// Number of assistant LLM-call spans, matching the trace viewer's span
    /// model. `None` for sessions persisted before this field existed; the
    /// dashboard falls back to `turns` for those until the next save.
    #[serde(default)]
    pub span_count: Option<u32>,
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
            provider: String::new(),
            thinking_level: None,
            title: None,
            source: String::new(),
            turns: 0,
            message_count: 0,
            context_tokens: 0,
            context_budget: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            span_count: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
}

// ---------------------------------------------------------------------------
// ListSessions — query for listing sessions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListSessions {
    pub limit: usize,
    /// Rows to skip before the first returned one (newest-first paging).
    #[serde(default)]
    pub offset: usize,
}
