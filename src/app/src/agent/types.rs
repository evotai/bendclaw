//! Shared data types for the agent module.

use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

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
// UsageSummary — token usage statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
}

impl UsageSummary {
    /// Cache hit rate as a fraction (0.0–1.0).
    pub fn cache_hit_rate(&self) -> f64 {
        let total_input = self.input + self.cache_read + self.cache_write;
        if total_input == 0 {
            return 0.0;
        }
        self.cache_read as f64 / total_input as f64
    }
}

// ---------------------------------------------------------------------------
// LlmCallMetrics — timing metrics for a single LLM streaming call
// ---------------------------------------------------------------------------

/// Timing metrics for a single LLM streaming call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmCallMetrics {
    /// Total wall-clock time (ms).
    pub duration_ms: u64,
    /// Time to first byte — request start to stream start (ms).
    pub ttfb_ms: u64,
    /// Time to first token — request start to first text/thinking delta (ms).
    pub ttft_ms: u64,
    /// Streaming duration — first delta to completion (ms).
    pub streaming_ms: u64,
    /// Number of delta chunks received.
    pub chunk_count: u64,
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
// TranscriptItem — a single item in a conversation transcript
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptItem {
    User {
        text: String,
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
// SessionMeta — session metadata
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// ListSessions — query for listing sessions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListSessions {
    pub limit: usize,
}
