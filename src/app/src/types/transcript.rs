//! Transcript domain model — items, entries, and context projection.

use serde::Deserialize;
use serde::Serialize;

use super::metrics::UsageSummary;

// ---------------------------------------------------------------------------
// AssistantBlock — content blocks in assistant messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantBlock {
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Thinking {
        text: String,
    },
}

// ---------------------------------------------------------------------------
// ToolCallRecord — tool call in a transcript
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

// ---------------------------------------------------------------------------
// TranscriptUserContent — user content blocks preserved in original order
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptUserContent {
    Text { text: String },
    Image { data: String, mime_type: String },
}

// ---------------------------------------------------------------------------
// TranscriptItem — a single item in a conversation transcript
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptItem {
    User {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        content: Vec<TranscriptUserContent>,
    },
    Assistant {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking: Option<String>,
        #[serde(default)]
        tool_calls: Vec<ToolCallRecord>,
        stop_reason: String,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: String,
        is_error: bool,
    },
    System {
        text: String,
    },
    Extension {
        kind: String,
        data: serde_json::Value,
    },
    Compact {
        messages: Vec<TranscriptItem>,
    },
    /// Observability fact — persisted in transcript.jsonl but never enters
    /// the conversation context sent to the engine.
    Stats {
        kind: String,
        data: serde_json::Value,
    },
}

impl TranscriptItem {
    /// Whether this item belongs in the conversation context view.
    ///
    /// Items that return `false` are observability/control facts that live in
    /// the raw transcript but must be filtered out before sending to the engine.
    pub fn is_context_item(&self) -> bool {
        !matches!(self, Self::Stats { .. } | Self::Compact { .. })
    }

    /// Build a User transcript item from engine content blocks.
    pub fn user_from_content(content: &[evot_engine::Content]) -> Self {
        let text = content
            .iter()
            .filter_map(|c| match c {
                evot_engine::Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        let content = content
            .iter()
            .filter_map(|c| match c {
                evot_engine::Content::Text { text } => {
                    Some(TranscriptUserContent::Text { text: text.clone() })
                }
                evot_engine::Content::Image { data, mime_type } => {
                    Some(TranscriptUserContent::Image {
                        data: data.clone(),
                        mime_type: mime_type.clone(),
                    })
                }
                _ => None,
            })
            .collect();

        Self::User { text, content }
    }
}

// ---------------------------------------------------------------------------
// TranscriptEntry — a transcript item with metadata for storage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub session_id: String,
    pub run_id: Option<String>,
    pub seq: u64,
    pub turn: u32,
    pub item: TranscriptItem,
    pub created_at: String,
}

impl TranscriptEntry {
    pub fn new(
        session_id: String,
        run_id: Option<String>,
        seq: u64,
        turn: u32,
        item: TranscriptItem,
    ) -> Self {
        Self {
            session_id,
            run_id,
            seq,
            turn,
            item,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// ListTranscriptEntries — query for listing transcript entries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTranscriptEntries {
    pub session_id: String,
    pub run_id: Option<String>,
    pub after_seq: Option<u64>,
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a run-finished usage summary by summing assistant-message usage.
/// This is a convenience re-export so callers don't need to depend on metrics
/// directly when they already have a `UsageSummary`.
pub fn empty_usage() -> UsageSummary {
    UsageSummary::default()
}
