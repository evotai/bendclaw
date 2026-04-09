//! Event system — RunEvent, RunEventPayload, ProtocolEvent, RunEventContext.

use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use super::types::AssistantBlock;
use super::types::LlmCallMetrics;
use super::types::TranscriptItem;
use super::types::UsageSummary;

// ---------------------------------------------------------------------------
// RunEventPayload — strongly typed event payload
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEventPayload {
    RunStarted {},
    TurnStarted {},
    AssistantDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        delta: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking_delta: Option<String>,
    },
    AssistantCompleted {
        content: Vec<AssistantBlock>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<UsageSummary>,
        stop_reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error_message: Option<String>,
    },
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        preview_command: Option<String>,
    },
    ToolProgress {
        tool_call_id: String,
        tool_name: String,
        text: String,
    },
    ToolFinished {
        tool_call_id: String,
        tool_name: String,
        content: String,
        is_error: bool,
        #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
        details: serde_json::Value,
        /// Estimated token count of the tool result content.
        #[serde(default)]
        result_tokens: usize,
        /// Wall-clock execution time (ms).
        #[serde(default)]
        duration_ms: u64,
    },
    LlmCallStarted {
        turn: usize,
        attempt: usize,
        model: String,
        system_prompt: String,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        message_count: usize,
        message_bytes: usize,
        system_prompt_tokens: usize,
    },
    LlmCallCompleted {
        turn: usize,
        attempt: usize,
        usage: UsageSummary,
        #[serde(default)]
        cache_read: u64,
        #[serde(default)]
        cache_write: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metrics: Option<LlmCallMetrics>,
    },
    ContextCompactionStarted {
        message_count: usize,
        estimated_tokens: usize,
        budget_tokens: usize,
        system_prompt_tokens: usize,
        context_window: usize,
    },
    ContextCompactionCompleted {
        level: u8,
        before_message_count: usize,
        after_message_count: usize,
        before_estimated_tokens: usize,
        after_estimated_tokens: usize,
        tool_outputs_truncated: usize,
        turns_summarized: usize,
        messages_dropped: usize,
        #[serde(default)]
        before_tool_details: Vec<(String, usize)>,
        #[serde(default)]
        after_tool_details: Vec<(String, usize)>,
    },
    RunFinished {
        text: String,
        usage: UsageSummary,
        turn_count: u32,
        duration_ms: u64,
        transcript_count: usize,
    },
    Error {
        message: String,
    },
}

impl RunEventPayload {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::TurnStarted { .. } => "turn_started",
            Self::AssistantDelta { .. } => "assistant_delta",
            Self::AssistantCompleted { .. } => "assistant_completed",
            Self::ToolStarted { .. } => "tool_started",
            Self::ToolProgress { .. } => "tool_progress",
            Self::ToolFinished { .. } => "tool_finished",
            Self::LlmCallStarted { .. } => "llm_call_started",
            Self::LlmCallCompleted { .. } => "llm_call_completed",
            Self::ContextCompactionStarted { .. } => "context_compaction_started",
            Self::ContextCompactionCompleted { .. } => "context_compaction_completed",
            Self::RunFinished { .. } => "run_finished",
            Self::Error { .. } => "error",
        }
    }
}

// ---------------------------------------------------------------------------
// RunEvent — custom serde to maintain { kind, payload: {...}, ... } shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RunEvent {
    pub event_id: String,
    pub run_id: String,
    pub session_id: String,
    pub turn: u32,
    pub payload: RunEventPayload,
    pub created_at: String,
}

impl RunEvent {
    pub fn new(run_id: String, session_id: String, turn: u32, payload: RunEventPayload) -> Self {
        Self {
            event_id: crate::ids::new_id(),
            run_id,
            session_id,
            turn,
            payload,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    pub fn kind_str(&self) -> &'static str {
        self.payload.kind_str()
    }
}

// Custom Serialize: output { event_id, run_id, session_id, turn, kind, payload, created_at }
impl Serialize for RunEvent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        // Serialize payload to Value, then strip the "kind" tag from it
        let payload_value =
            serde_json::to_value(&self.payload).map_err(serde::ser::Error::custom)?;
        let payload_obj = match &payload_value {
            serde_json::Value::Object(map) => {
                let mut stripped = serde_json::Map::new();
                for (k, v) in map {
                    if k != "kind" {
                        stripped.insert(k.clone(), v.clone());
                    }
                }
                serde_json::Value::Object(stripped)
            }
            other => other.clone(),
        };

        let mut map = serializer.serialize_map(Some(7))?;
        map.serialize_entry("event_id", &self.event_id)?;
        map.serialize_entry("run_id", &self.run_id)?;
        map.serialize_entry("session_id", &self.session_id)?;
        map.serialize_entry("turn", &self.turn)?;
        map.serialize_entry("kind", self.kind_str())?;
        map.serialize_entry("payload", &payload_obj)?;
        map.serialize_entry("created_at", &self.created_at)?;
        map.end()
    }
}

// Custom Deserialize: read kind, then use it to deserialize payload
impl<'de> Deserialize<'de> for RunEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let obj = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("expected object"))?;

        let event_id = obj
            .get("event_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::missing_field("event_id"))?
            .to_string();
        let run_id = obj
            .get("run_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::missing_field("run_id"))?
            .to_string();
        let session_id = obj
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::missing_field("session_id"))?
            .to_string();
        let turn_u64 = obj
            .get("turn")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| serde::de::Error::missing_field("turn"))?;
        let turn = u32::try_from(turn_u64).map_err(|_| {
            serde::de::Error::custom(format!("turn value {turn_u64} exceeds u32 range"))
        })?;
        let created_at = obj
            .get("created_at")
            .and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::missing_field("created_at"))?
            .to_string();

        // Reconstruct payload by injecting kind back into the payload object
        let kind_str = obj
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::missing_field("kind"))?;
        let payload_value = obj
            .get("payload")
            .ok_or_else(|| serde::de::Error::missing_field("payload"))?
            .clone();
        let tagged = match payload_value {
            serde_json::Value::Object(mut map) => {
                map.insert(
                    "kind".to_string(),
                    serde_json::Value::String(kind_str.to_string()),
                );
                serde_json::Value::Object(map)
            }
            other => other,
        };
        let payload: RunEventPayload =
            serde_json::from_value(tagged).map_err(serde::de::Error::custom)?;

        Ok(RunEvent {
            event_id,
            run_id,
            session_id,
            turn,
            payload,
            created_at,
        })
    }
}

// ---------------------------------------------------------------------------
// ProtocolEvent — app-layer abstraction of engine events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ProtocolEvent {
    AgentStart,
    AgentEnd {
        transcripts: Vec<TranscriptItem>,
        usage: UsageSummary,
        transcript_count: usize,
    },
    TurnStart,
    TurnEnd,
    AssistantDelta {
        delta: Option<String>,
        thinking_delta: Option<String>,
    },
    AssistantCompleted {
        content: Vec<AssistantBlock>,
        usage: Option<UsageSummary>,
        stop_reason: String,
        error_message: Option<String>,
    },
    ToolStart {
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
        preview_command: Option<String>,
    },
    ToolProgress {
        tool_call_id: String,
        tool_name: String,
        text: String,
    },
    ToolEnd {
        tool_call_id: String,
        tool_name: String,
        content: String,
        is_error: bool,
        details: serde_json::Value,
        /// Estimated token count of the tool result content.
        result_tokens: usize,
        /// Wall-clock execution time (ms).
        duration_ms: u64,
    },
    LlmCallStart {
        turn: usize,
        attempt: usize,
        model: String,
        system_prompt: String,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        message_count: usize,
        system_prompt_tokens: usize,
    },
    LlmCallEnd {
        turn: usize,
        attempt: usize,
        usage: UsageSummary,
        error: Option<String>,
        metrics: Option<LlmCallMetrics>,
    },
    /// Unified error event from the engine.
    /// Replaces the former `InputRejected` variant.
    Error {
        kind: String,
        message: String,
    },
    ContextCompactionStart {
        message_count: usize,
        estimated_tokens: usize,
        budget_tokens: usize,
        system_prompt_tokens: usize,
        context_window: usize,
    },
    ContextCompactionEnd {
        level: u8,
        before_message_count: usize,
        after_message_count: usize,
        before_estimated_tokens: usize,
        after_estimated_tokens: usize,
        tool_outputs_truncated: usize,
        turns_summarized: usize,
        messages_dropped: usize,
        before_tool_details: Vec<(String, usize)>,
        after_tool_details: Vec<(String, usize)>,
        compacted_transcripts: Vec<TranscriptItem>,
    },
}

// ---------------------------------------------------------------------------
// RunEventContext — ProtocolEvent → RunEvent conversion (pure model→model)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct RunEventContext<'a> {
    run_id: &'a str,
    session_id: &'a str,
    turn: u32,
}

impl<'a> RunEventContext<'a> {
    pub fn new(run_id: &'a str, session_id: &'a str, turn: u32) -> Self {
        Self {
            run_id,
            session_id,
            turn,
        }
    }

    pub fn started(&self) -> RunEvent {
        self.with_turn(0).event(RunEventPayload::RunStarted {})
    }

    pub fn finished(
        &self,
        text: String,
        usage: UsageSummary,
        turn_count: u32,
        duration_ms: u64,
        transcript_count: usize,
    ) -> RunEvent {
        self.event(RunEventPayload::RunFinished {
            text,
            usage,
            turn_count,
            duration_ms,
            transcript_count,
        })
    }

    pub fn map(&self, event: &ProtocolEvent) -> Option<RunEvent> {
        let payload = match event {
            ProtocolEvent::AgentStart => return None,
            ProtocolEvent::AgentEnd { .. } => return None,
            ProtocolEvent::TurnStart => RunEventPayload::TurnStarted {},
            ProtocolEvent::TurnEnd => return None,
            ProtocolEvent::AssistantDelta {
                delta,
                thinking_delta,
            } => RunEventPayload::AssistantDelta {
                delta: delta.clone(),
                thinking_delta: thinking_delta.clone(),
            },
            ProtocolEvent::AssistantCompleted {
                content,
                usage,
                stop_reason,
                error_message,
            } => RunEventPayload::AssistantCompleted {
                content: content.clone(),
                usage: usage.clone(),
                stop_reason: stop_reason.clone(),
                error_message: error_message.clone(),
            },
            ProtocolEvent::ToolStart {
                tool_call_id,
                tool_name,
                args,
                preview_command,
            } => RunEventPayload::ToolStarted {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                args: args.clone(),
                preview_command: preview_command.clone(),
            },
            ProtocolEvent::ToolProgress {
                tool_call_id,
                tool_name,
                text,
            } => RunEventPayload::ToolProgress {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                text: text.clone(),
            },
            ProtocolEvent::ToolEnd {
                tool_call_id,
                tool_name,
                content,
                is_error,
                details,
                result_tokens,
                duration_ms,
            } => RunEventPayload::ToolFinished {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                content: content.clone(),
                is_error: *is_error,
                details: details.clone(),
                result_tokens: *result_tokens,
                duration_ms: *duration_ms,
            },
            ProtocolEvent::Error { message, .. } => RunEventPayload::Error {
                message: message.clone(),
            },
            ProtocolEvent::LlmCallStart {
                turn,
                attempt,
                model,
                system_prompt,
                messages,
                tools,
                message_count,
                system_prompt_tokens,
            } => {
                let message_bytes: usize = messages.iter().map(|m| m.to_string().len()).sum();
                RunEventPayload::LlmCallStarted {
                    turn: *turn,
                    attempt: *attempt,
                    model: model.clone(),
                    system_prompt: system_prompt.clone(),
                    messages: messages.clone(),
                    tools: tools.clone(),
                    message_count: *message_count,
                    message_bytes,
                    system_prompt_tokens: *system_prompt_tokens,
                }
            }
            ProtocolEvent::LlmCallEnd {
                turn,
                attempt,
                usage,
                error,
                metrics,
            } => RunEventPayload::LlmCallCompleted {
                turn: *turn,
                attempt: *attempt,
                usage: usage.clone(),
                cache_read: usage.cache_read,
                cache_write: usage.cache_write,
                error: error.clone(),
                metrics: metrics.clone(),
            },
            ProtocolEvent::ContextCompactionStart {
                message_count,
                estimated_tokens,
                budget_tokens,
                system_prompt_tokens,
                context_window,
            } => RunEventPayload::ContextCompactionStarted {
                message_count: *message_count,
                estimated_tokens: *estimated_tokens,
                budget_tokens: *budget_tokens,
                system_prompt_tokens: *system_prompt_tokens,
                context_window: *context_window,
            },
            ProtocolEvent::ContextCompactionEnd {
                level,
                before_message_count,
                after_message_count,
                before_estimated_tokens,
                after_estimated_tokens,
                tool_outputs_truncated,
                turns_summarized,
                messages_dropped,
                before_tool_details,
                after_tool_details,
                compacted_transcripts: _,
            } => RunEventPayload::ContextCompactionCompleted {
                level: *level,
                before_message_count: *before_message_count,
                after_message_count: *after_message_count,
                before_estimated_tokens: *before_estimated_tokens,
                after_estimated_tokens: *after_estimated_tokens,
                tool_outputs_truncated: *tool_outputs_truncated,
                turns_summarized: *turns_summarized,
                messages_dropped: *messages_dropped,
                before_tool_details: before_tool_details.clone(),
                after_tool_details: after_tool_details.clone(),
            },
        };

        Some(self.event(payload))
    }

    fn with_turn(self, turn: u32) -> Self {
        Self { turn, ..self }
    }

    fn event(&self, payload: RunEventPayload) -> RunEvent {
        RunEvent::new(
            self.run_id.to_string(),
            self.session_id.to_string(),
            self.turn,
            payload,
        )
    }
}
