use serde::Deserialize;
use serde::Serialize;

use crate::kernel::run::result::Reason;
use crate::kernel::run::result::Usage;
use crate::kernel::tools::cli_agent::AgentEvent;
use crate::kernel::OperationMeta;
use crate::llm::stream::StreamEvent;
use crate::llm::usage::TokenUsage;

/// Fine-grained lifecycle events emitted by the agent loop.
///
/// Every state-machine step produces an event — no silent operations.
/// Consumers (SSE, audit, logging, UI) subscribe to these via the channel
/// returned by `Session::run()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Event {
    /// The agent loop has started.
    Start,

    /// The agent loop has finished.
    End {
        iterations: u32,
        stop_reason: String,
        usage: Usage,
    },

    /// A new reasoning turn begins.
    TurnStart { iteration: u32 },

    /// A reasoning turn has ended.
    TurnEnd { iteration: u32 },

    /// The LLM is being called.
    ReasonStart,

    /// A streaming chunk from the LLM (text, thinking, or tool call fragment).
    StreamDelta(Delta),

    /// The LLM response is complete for this turn.
    ReasonEnd { finish_reason: String },

    /// The LLM stream failed.
    ReasonError { error: String },

    /// A tool execution has started.
    ToolStart {
        tool_call_id: String,
        name: String,
        arguments: serde_json::Value,
    },

    /// Partial output from a running tool.
    ToolUpdate {
        tool_call_id: String,
        event: AgentEvent,
    },

    /// A tool execution has finished.
    ToolEnd {
        tool_call_id: String,
        name: String,
        success: bool,
        output: String,
        operation: OperationMeta,
    },

    /// Context compaction completed.
    CompactionDone {
        messages_before: usize,
        messages_after: usize,
        summary_len: usize,
    },

    /// Pre-compaction checkpoint (memory persistence) completed.
    CheckpointDone {
        prompt_tokens: u64,
        completion_tokens: u64,
    },

    /// The loop was aborted (timeout, max iterations, or cancellation).
    Aborted { reason: Reason },

    /// Something went wrong.
    Error { message: String },

    /// Named audit event persisted into `run_events`.
    Audit {
        name: String,
        payload: serde_json::Value,
    },

    /// Application-specific data (plan updates, step results, etc.).
    AppData(serde_json::Value),

    /// Progress notification from a running tool, broadcast to external subscribers.
    Progress {
        tool_call_id: Option<String>,
        message: String,
    },

    /// A user message was injected into the running engine via the inbox channel.
    MessageInjected { content: String },
}

impl Event {
    /// Returns a string name for the event variant.
    ///
    /// Most variants return their type name (e.g. `"Start"`, `"ToolEnd"`).
    /// The `Audit` variant is special: it returns the *event name field* rather
    /// than the literal string `"Audit"`, so that audit events can carry
    /// domain-specific names like `"llm.request"` or `"run.started"` that are
    /// stored directly in `run_events.event`.
    pub fn name(&self) -> String {
        match self {
            Self::Start => "Start".to_string(),
            Self::End { .. } => "End".to_string(),
            Self::TurnStart { .. } => "TurnStart".to_string(),
            Self::TurnEnd { .. } => "TurnEnd".to_string(),
            Self::ReasonStart => "ReasonStart".to_string(),
            Self::StreamDelta(_) => "StreamDelta".to_string(),
            Self::ReasonEnd { .. } => "ReasonEnd".to_string(),
            Self::ReasonError { .. } => "ReasonError".to_string(),
            Self::ToolStart { .. } => "ToolStart".to_string(),
            Self::ToolUpdate { .. } => "ToolUpdate".to_string(),
            Self::ToolEnd { .. } => "ToolEnd".to_string(),
            Self::CompactionDone { .. } => "CompactionDone".to_string(),
            Self::CheckpointDone { .. } => "CheckpointDone".to_string(),
            Self::Aborted { .. } => "Aborted".to_string(),
            Self::Error { .. } => "Error".to_string(),
            Self::Audit { name, .. } => name.clone(),
            Self::AppData(_) => "AppData".to_string(),
            Self::Progress { .. } => "Progress".to_string(),
            Self::MessageInjected { .. } => "MessageInjected".to_string(),
        }
    }
}

/// A streaming chunk from the LLM, mapped from `StreamEvent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Delta {
    Text {
        content: String,
    },
    Thinking {
        content: String,
    },
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: usize,
        json_chunk: String,
    },
    ToolCallEnd {
        index: usize,
        id: String,
        name: String,
        arguments: String,
    },
    Usage(TokenUsage),
    Done {
        finish_reason: String,
        provider: Option<String>,
        model: Option<String>,
    },
}

impl Delta {
    /// Convert an LLM `StreamEvent` into an `Event::StreamDelta`.
    /// Returns `None` for events that don't map to deltas (Done, Error).
    pub fn from_stream_event(event: &StreamEvent) -> Option<Self> {
        match event {
            StreamEvent::ContentDelta(s) => Some(Self::Text { content: s.clone() }),
            StreamEvent::ThinkingDelta(s) => Some(Self::Thinking { content: s.clone() }),
            StreamEvent::ToolCallStart { index, id, name } => Some(Self::ToolCallStart {
                index: *index,
                id: id.clone(),
                name: name.clone(),
            }),
            StreamEvent::ToolCallDelta { index, json_chunk } => Some(Self::ToolCallDelta {
                index: *index,
                json_chunk: json_chunk.clone(),
            }),
            StreamEvent::ToolCallEnd {
                index,
                id,
                name,
                arguments,
            } => Some(Self::ToolCallEnd {
                index: *index,
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            }),
            StreamEvent::Usage(u) => Some(Self::Usage(u.clone())),
            StreamEvent::Done {
                finish_reason,
                provider,
                model,
            } => Some(Self::Done {
                finish_reason: finish_reason.clone(),
                provider: provider.clone(),
                model: model.clone(),
            }),
            StreamEvent::Error(_) => None,
        }
    }
}
