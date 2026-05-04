use serde::Deserialize;
use serde::Serialize;

use crate::types::AgentMessage;

/// Per-tool token breakdown entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolTokenDetail {
    pub tool_name: String,
    pub tokens: usize,
}

/// Describes what happened to a single item during compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionAction {
    /// Message index in the original list (0-based).
    pub index: usize,
    /// Tool name, "assistant", or "messages".
    pub tool_name: String,
    /// What method was used.
    pub method: CompactionMethod,
    /// Tokens before compaction.
    pub before_tokens: usize,
    /// Tokens after compaction.
    pub after_tokens: usize,
    /// End index for range actions (evict).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<usize>,
    /// Count of related messages (e.g. tool results in a collapsed turn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_count: Option<usize>,
}

/// The method used to compact a message or tool result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompactionMethod {
    /// CurrentRun result reclaimed after its lifecycle ended.
    #[serde(alias = "LifecycleCleared")]
    LifecycleReclaimed,
    /// Oversized result capped.
    #[serde(alias = "oversize_capped")]
    OversizeCapped,
    /// Old result cleared by age policy.
    #[serde(alias = "age_cleared")]
    AgeCleared,
    /// Head + tail truncation.
    HeadTail,
    /// Tree-sitter structural outline extraction.
    Outline,
    /// Old image stripped under severe pressure.
    ImageStripped,
    /// Assistant turn collapsed into a summary.
    #[serde(alias = "Summarized")]
    TurnCollapsed,
    /// Messages evicted from stale context.
    #[serde(alias = "Dropped")]
    MessagesEvicted,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionStats {
    /// User-visible compaction level: 3=evict, 2=collapse, 1=shrink, 0=reclaim/no-op.
    pub level: u8,
    pub before_message_count: usize,
    pub after_message_count: usize,
    pub before_estimated_tokens: usize,
    pub after_estimated_tokens: usize,
    pub tool_outputs_truncated: usize,
    pub turns_summarized: usize,
    pub messages_dropped: usize,
    pub current_run_cleared: usize,
    /// Count of oversized results capped.
    #[serde(default)]
    pub oversize_capped: usize,
    /// Count of old results/images cleared by age policy.
    #[serde(default)]
    pub age_cleared: usize,
    /// Per-tool token breakdown before compaction (sorted by tokens desc).
    #[serde(default)]
    pub before_tool_details: Vec<ToolTokenDetail>,
    /// Per-tool token breakdown after compaction (sorted by tokens desc).
    #[serde(default)]
    pub after_tool_details: Vec<ToolTokenDetail>,
    /// Per-message compaction actions.
    #[serde(default)]
    pub actions: Vec<CompactionAction>,
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub messages: Vec<AgentMessage>,
    pub stats: CompactionStats,
}
