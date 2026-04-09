//! Observability model — structured stats for transcript persistence.
//!
//! Each stats struct represents a single observability fact produced during
//! a run. `TranscriptStats` is the aggregating enum that provides encoding
//! (`to_item`) and decoding (`try_from_item`) between the strong types and
//! the flat `TranscriptItem::Stats { kind, data }` storage representation.

use serde::Deserialize;
use serde::Serialize;

use super::metrics::LlmCallMetrics;
use super::metrics::UsageSummary;
use super::transcript::TranscriptItem;

// ---------------------------------------------------------------------------
// Stats structs — one per observability event kind
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmCallStartedStats {
    pub turn: usize,
    pub attempt: usize,
    pub model: String,
    pub message_count: usize,
    pub message_bytes: usize,
    pub system_prompt_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmCallCompletedStats {
    pub turn: usize,
    pub attempt: usize,
    pub usage: UsageSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<LlmCallMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFinishedStats {
    pub tool_call_id: String,
    pub tool_name: String,
    pub result_tokens: usize,
    pub duration_ms: u64,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompactionStartedStats {
    pub message_count: usize,
    pub estimated_tokens: usize,
    pub budget_tokens: usize,
    pub system_prompt_tokens: usize,
    pub context_window: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompactionCompletedStats {
    pub level: u8,
    pub before_message_count: usize,
    pub after_message_count: usize,
    pub before_estimated_tokens: usize,
    pub after_estimated_tokens: usize,
    pub tool_outputs_truncated: usize,
    pub turns_summarized: usize,
    pub messages_dropped: usize,
    #[serde(default)]
    pub actions: Vec<CompactionActionStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionActionStats {
    pub index: usize,
    pub tool_name: String,
    pub method: String,
    pub before_tokens: usize,
    pub after_tokens: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunFinishedStats {
    pub usage: UsageSummary,
    pub turn_count: u32,
    pub duration_ms: u64,
    pub transcript_count: usize,
}

// ---------------------------------------------------------------------------
// TranscriptStats — aggregating enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum TranscriptStats {
    LlmCallStarted(LlmCallStartedStats),
    LlmCallCompleted(LlmCallCompletedStats),
    ToolFinished(ToolFinishedStats),
    ContextCompactionStarted(ContextCompactionStartedStats),
    ContextCompactionCompleted(ContextCompactionCompletedStats),
    RunFinished(RunFinishedStats),
}

impl TranscriptStats {
    /// Stable kind string for serialization / grep / jq.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::LlmCallStarted(_) => "llm_call_started",
            Self::LlmCallCompleted(_) => "llm_call_completed",
            Self::ToolFinished(_) => "tool_finished",
            Self::ContextCompactionStarted(_) => "context_compaction_started",
            Self::ContextCompactionCompleted(_) => "context_compaction_completed",
            Self::RunFinished(_) => "run_finished",
        }
    }

    /// Convert to a flat `TranscriptItem::Stats` for persistence.
    pub fn to_item(&self) -> TranscriptItem {
        let kind = self.kind_str().to_string();
        let data = match self {
            Self::LlmCallStarted(s) => serde_json::to_value(s),
            Self::LlmCallCompleted(s) => serde_json::to_value(s),
            Self::ToolFinished(s) => serde_json::to_value(s),
            Self::ContextCompactionStarted(s) => serde_json::to_value(s),
            Self::ContextCompactionCompleted(s) => serde_json::to_value(s),
            Self::RunFinished(s) => serde_json::to_value(s),
        }
        .unwrap_or_default();
        TranscriptItem::Stats { kind, data }
    }

    /// Try to decode a `TranscriptItem::Stats` back into a strong type.
    ///
    /// Returns `None` for non-Stats items, unknown kinds, or schema mismatches.
    pub fn try_from_item(item: &TranscriptItem) -> Option<Self> {
        let (kind, data) = match item {
            TranscriptItem::Stats { kind, data } => (kind.as_str(), data),
            _ => return None,
        };
        match kind {
            "llm_call_started" => serde_json::from_value(data.clone())
                .ok()
                .map(Self::LlmCallStarted),
            "llm_call_completed" => serde_json::from_value(data.clone())
                .ok()
                .map(Self::LlmCallCompleted),
            "tool_finished" => serde_json::from_value(data.clone())
                .ok()
                .map(Self::ToolFinished),
            "context_compaction_started" => serde_json::from_value(data.clone())
                .ok()
                .map(Self::ContextCompactionStarted),
            "context_compaction_completed" => serde_json::from_value(data.clone())
                .ok()
                .map(Self::ContextCompactionCompleted),
            "run_finished" => serde_json::from_value(data.clone())
                .ok()
                .map(Self::RunFinished),
            _ => None,
        }
    }
}
