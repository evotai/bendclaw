use serde::Deserialize;
use serde::Serialize;

use super::llm::LlmCallMetrics;
use super::llm::Usage;
use super::message::AgentMessage;
use super::message::Message;
use super::tool::ToolResult;
use crate::provider::ToolDefinition;

// ---------------------------------------------------------------------------
// Unified error model
// ---------------------------------------------------------------------------

/// Classification of agent errors by source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentErrorKind {
    /// LLM provider error (API failure, rate limit, etc.)
    Provider,
    /// Agent runtime error (bad state, missing context, etc.)
    Runtime,
    /// Input rejected by a filter.
    InputRejected,
}

/// Structured error information for `AgentEvent::Error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentErrorInfo {
    pub kind: AgentErrorKind,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Agent events
// ---------------------------------------------------------------------------

pub enum AgentEvent {
    AgentStart,
    AgentEnd {
        messages: Vec<AgentMessage>,
    },
    TurnStart,
    TurnEnd {
        message: AgentMessage,
        tool_results: Vec<Message>,
    },
    MessageStart {
        message: AgentMessage,
    },
    MessageUpdate {
        message: AgentMessage,
        delta: StreamDelta,
    },
    MessageEnd {
        message: AgentMessage,
    },
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
        preview_command: Option<String>,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        partial_result: ToolResult,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: ToolResult,
        is_error: bool,
        /// Estimated token count for the tool result content.
        result_tokens: usize,
        /// Wall-clock execution time (ms).
        duration_ms: u64,
    },
    ProgressMessage {
        tool_call_id: String,
        tool_name: String,
        text: String,
    },
    /// Unified error event — the single channel for all user-visible errors.
    ///
    /// Replaces the former `InputRejected` variant and provider/runtime `warn!` logs.
    /// App layer should display this to the user but NOT write it to transcript.
    Error {
        error: AgentErrorInfo,
    },
    LlmCallStart {
        turn: usize,
        attempt: usize,
        injected_count: usize,
        request: LlmCallRequest,
        /// Pre-computed message stats from structured Content types.
        stats: LlmCallStats,
        /// Context budget snapshot (same source as compaction events).
        budget: crate::context::ContextBudgetSnapshot,
        /// OTel: standardized provider name (e.g. "anthropic", "aws.bedrock", "openai").
        provider_name: String,
        /// OTel: server address extracted from base_url.
        server_address: Option<String>,
        /// OTel: server port extracted from base_url.
        server_port: Option<u16>,
    },
    LlmCallEnd {
        turn: usize,
        attempt: usize,
        usage: Usage,
        error: Option<String>,
        metrics: LlmCallMetrics,
        context_window: usize,
        /// Stop reason from the LLM response (for `gen_ai.response.finish_reasons`).
        stop_reason: super::llm::StopReason,
        /// Response content blocks (text + tool calls). Empty on error.
        /// Used for `gen_ai.output.messages` (Opt-In) and verbose UI.
        content: Vec<super::message::Content>,
        /// OTel: actual model name from the provider response.
        response_model: Option<String>,
        /// OTel: unique completion identifier from the provider (e.g. `chatcmpl-xxx`, `msg_xxx`).
        response_id: Option<String>,
    },
    ContextCompactionStart {
        message_count: usize,
        /// Context budget snapshot at the time of compaction.
        budget: crate::context::ContextBudgetSnapshot,
        /// Pre-computed message stats for the context being compacted.
        message_stats: LlmCallStats,
    },
    ContextCompactionEnd {
        stats: crate::context::CompactionStats,
        messages: Vec<AgentMessage>,
        context_window: usize,
    },
}

// ---------------------------------------------------------------------------
// LLM call request snapshot
// ---------------------------------------------------------------------------

/// Canonical snapshot of the input sent to the LLM provider for a single call.
#[derive(Debug, Clone)]
pub struct LlmCallRequest {
    pub model: String,
    pub system_prompt: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

/// Pre-computed message stats for an LLM call, computed at the engine layer
/// from structured `Message`/`Content` types for accurate token accounting.
///
/// Does NOT include `message_count` or `tool_count` — those come from
/// `request.messages.len()` / `request.tools.len()` to avoid dual sources.
#[derive(Debug, Clone, Default)]
pub struct LlmCallStats {
    pub user_count: usize,
    pub assistant_count: usize,
    pub tool_result_count: usize,
    pub image_count: usize,
    pub image_path_count: usize,
    pub image_base64_count: usize,
    pub user_tokens: usize,
    pub assistant_tokens: usize,
    pub tool_result_tokens: usize,
    pub image_tokens: usize,
    /// Per-tool token breakdown: (name, estimated_tokens), sorted desc.
    pub tool_details: Vec<(String, usize)>,
}

#[derive(Debug, Clone)]
pub enum StreamDelta {
    Text { delta: String },
    Thinking { delta: String },
    ToolCallDelta { delta: String },
}
