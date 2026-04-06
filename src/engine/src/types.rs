use std::fmt;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

use crate::provider::ToolDefinition;

// ---------------------------------------------------------------------------
// Content types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Content {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    #[serde(rename = "toolCall")]
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "user")]
    User {
        content: Vec<Content>,
        timestamp: u64,
    },
    #[serde(rename = "assistant")]
    Assistant {
        content: Vec<Content>,
        #[serde(rename = "stopReason")]
        stop_reason: StopReason,
        model: String,
        provider: String,
        usage: Usage,
        timestamp: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_message: Option<String>,
    },
    #[serde(rename = "toolResult")]
    ToolResult {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        content: Vec<Content>,
        #[serde(rename = "isError")]
        is_error: bool,
        timestamp: u64,
    },
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self::User {
            content: vec![Content::Text { text: text.into() }],
            timestamp: now_ms(),
        }
    }

    pub fn role(&self) -> &str {
        match self {
            Self::User { .. } => "user",
            Self::Assistant { .. } => "assistant",
            Self::ToolResult { .. } => "toolResult",
        }
    }

    /// Check if this assistant message represents a context overflow error.
    ///
    /// Some providers (SSE-based: Anthropic, OpenAI) return overflow as a
    /// `StopReason::Error` message rather than an HTTP error. This method
    /// checks the `error_message` field against known overflow patterns.
    pub fn is_context_overflow(&self) -> bool {
        match self {
            Self::Assistant {
                stop_reason: StopReason::Error,
                error_message: Some(msg),
                ..
            } => crate::provider::is_context_overflow_message(msg),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// AgentMessage — LLM messages + extensible custom types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionMessage {
    pub role: String,
    pub kind: String,
    pub data: serde_json::Value,
}

impl ExtensionMessage {
    pub fn new(kind: impl Into<String>, data: impl Serialize) -> Self {
        Self {
            role: "extension".into(),
            kind: kind.into(),
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AgentMessage {
    /// Standard LLM message
    Llm(Message),
    /// App-specific message (UI-only, notifications, etc.)
    Extension(ExtensionMessage),
}

impl AgentMessage {
    pub fn role(&self) -> &str {
        match self {
            Self::Llm(m) => m.role(),
            Self::Extension(ext) => &ext.role,
        }
    }

    pub fn as_llm(&self) -> Option<&Message> {
        match self {
            Self::Llm(m) => Some(m),
            Self::Extension(_) => None,
        }
    }
}

impl From<Message> for AgentMessage {
    fn from(m: Message) -> Self {
        Self::Llm(m)
    }
}

// ---------------------------------------------------------------------------
// Stop reasons & usage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    Stop,
    Length,
    ToolUse,
    Error,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

impl Usage {
    /// Fraction of input tokens served from cache (0.0–1.0).
    /// Returns 0.0 if no input tokens were processed.
    pub fn cache_hit_rate(&self) -> f64 {
        let total_input = self.input + self.cache_read + self.cache_write;
        if total_input == 0 {
            return 0.0;
        }
        self.cache_read as f64 / total_input as f64
    }
}

// ---------------------------------------------------------------------------
// Cache configuration
// ---------------------------------------------------------------------------

/// Controls prompt caching behavior for providers that support it.
///
/// By default, caching is enabled with automatic breakpoint placement.
/// This gives optimal cost savings without any user configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Master switch — set to false to disable all caching hints.
    /// Default: true.
    pub enabled: bool,
    /// How cache breakpoints are placed.
    pub strategy: CacheStrategy,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: CacheStrategy::Auto,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool execution strategy
// ---------------------------------------------------------------------------

/// Controls how multiple tool calls from a single LLM response are executed.
///
/// When the LLM returns multiple tool calls (e.g., "read file A, read file B,
/// run bash C"), this determines whether they run sequentially or in parallel.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToolExecutionStrategy {
    /// Run tools one at a time, check steering between each.
    /// Use for debugging or tools with shared mutable state.
    Sequential,
    /// Run all tool calls concurrently, check steering after all complete.
    /// Default — most tool calls are independent and this gives the best latency.
    #[default]
    Parallel,
    /// Run in batches of N, check steering between batches.
    /// Balances speed with human-in-the-loop control.
    Batched { size: usize },
}

/// Strategy for placing cache breakpoints (Anthropic-specific; other providers
/// handle caching automatically regardless of this setting).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CacheStrategy {
    /// Automatic breakpoint placement (recommended).
    /// Caches: system prompt, tool definitions, and recent conversation history.
    #[default]
    Auto,
    /// Disable caching entirely.
    Disabled,
    /// Fine-grained control over what gets cached.
    Manual {
        /// Cache the system prompt.
        cache_system: bool,
        /// Cache tool definitions.
        cache_tools: bool,
        /// Cache conversation history (second-to-last message).
        cache_messages: bool,
    },
}

// ---------------------------------------------------------------------------
// Thinking level
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    #[default]
    Off,
    Minimal,
    Low,
    Medium,
    High,
}

// ---------------------------------------------------------------------------
// Tool definition
// ---------------------------------------------------------------------------

/// Callback for streaming partial results during tool execution.
///
/// Tools call this to emit progress updates (e.g., partial output, status messages)
/// that are forwarded as `AgentEvent::ToolExecutionUpdate` events for UI consumption.
/// Partial results are **not** sent to the LLM — only the final `ToolResult` is.
pub type ToolUpdateFn = Arc<dyn Fn(ToolResult) + Send + Sync>;

/// Callback for emitting user-facing progress messages during tool execution.
///
/// Each invocation emits an `AgentEvent::ProgressMessage` event. Unlike `ToolUpdateFn`,
/// these are simple text messages intended for user-facing display (e.g., status lines,
/// notifications), not structured tool results.
pub type ProgressFn = Arc<dyn Fn(String) + Send + Sync>;

/// Context passed to tool execution. Bundles all per-invocation state.
///
/// Using a struct instead of individual parameters future-proofs the trait —
/// adding fields to `ToolContext` is non-breaking.
pub struct ToolContext {
    /// The ID of this tool call (for correlation).
    pub tool_call_id: String,
    /// The name of the tool being invoked.
    pub tool_name: String,
    /// Cancellation token — check `is_cancelled()` in long-running tools.
    pub cancel: tokio_util::sync::CancellationToken,
    /// Optional callback for streaming partial `ToolResult`s (UI/logging only).
    pub on_update: Option<ToolUpdateFn>,
    /// Optional callback for emitting user-facing progress messages.
    pub on_progress: Option<ProgressFn>,
}

impl Clone for ToolContext {
    fn clone(&self) -> Self {
        Self {
            tool_call_id: self.tool_call_id.clone(),
            tool_name: self.tool_name.clone(),
            cancel: self.cancel.clone(),
            on_update: self.on_update.clone(),
            on_progress: self.on_progress.clone(),
        }
    }
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("cancel", &self.cancel)
            .field("on_update", &self.on_update.as_ref().map(|_| "<callback>"))
            .field(
                "on_progress",
                &self.on_progress.as_ref().map(|_| "<callback>"),
            )
            .finish()
    }
}

/// A tool the agent can call. Implement this trait for your tools.
#[async_trait::async_trait]
pub trait AgentTool: Send + Sync {
    /// Unique tool name (used in LLM tool_use)
    fn name(&self) -> &str;
    /// Human-readable label for UI
    fn label(&self) -> &str;
    /// Description for the LLM
    fn description(&self) -> &str;
    /// JSON Schema for parameters
    fn parameters_schema(&self) -> serde_json::Value;
    /// Execute the tool.
    ///
    /// The `ctx` parameter provides per-invocation context:
    /// - `ctx.tool_call_id` / `ctx.tool_name` — for correlation and logging
    /// - `ctx.cancel` — cancellation token; check `is_cancelled()` in long-running tools
    /// - `ctx.on_update` — optional callback for streaming partial `ToolResult`s (UI/logging only)
    /// - `ctx.on_progress` — optional callback for user-facing progress text (`ProgressMessage`)
    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: Vec<Content>,
    #[serde(default)]
    pub details: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Failed(String),
    #[error("Tool not found: {0}")]
    NotFound(String),
    #[error("Invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("Cancelled")]
    Cancelled,
}

// ---------------------------------------------------------------------------
// Agent events (for streaming UI updates)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
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
    },
    ProgressMessage {
        tool_call_id: String,
        tool_name: String,
        text: String,
    },
    InputRejected {
        reason: String,
    },
    LlmCallStart {
        turn: usize,
        attempt: usize,
        request: LlmCallRequest,
    },
    LlmCallEnd {
        turn: usize,
        attempt: usize,
        usage: Usage,
        error: Option<String>,
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
}

#[derive(Debug, Clone)]
pub enum StreamDelta {
    Text { delta: String },
    Thinking { delta: String },
    ToolCallDelta { delta: String },
}

// ---------------------------------------------------------------------------
// Agent context (passed to the loop)
// ---------------------------------------------------------------------------

pub struct AgentContext {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<Box<dyn AgentTool>>,
}

// ---------------------------------------------------------------------------
// Input filtering
// ---------------------------------------------------------------------------

/// Result of applying an input filter to a user message.
#[derive(Debug, Clone)]
pub enum FilterResult {
    /// Message passes unchanged.
    Pass,
    /// Message passes, but append a warning to context for the LLM to see.
    Warn(String),
    /// Message is rejected. Agent loop returns immediately.
    Reject(String),
}

/// Synchronous filter applied to user input before the LLM call.
///
/// Implement this for injection detection, content moderation, PII redaction, etc.
/// Filters run in the hot path and must be fast — use `before_turn` callbacks
/// for async moderation (external API calls).
pub trait InputFilter: Send + Sync {
    fn filter(&self, text: &str) -> FilterResult;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stop => write!(f, "stop"),
            Self::Length => write!(f, "length"),
            Self::ToolUse => write!(f, "toolUse"),
            Self::Error => write!(f, "error"),
            Self::Aborted => write!(f, "aborted"),
        }
    }
}
