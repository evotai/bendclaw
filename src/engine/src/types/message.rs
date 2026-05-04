use serde::Deserialize;
use serde::Serialize;

use super::agent_context::now_ms;
use super::llm::StopReason;
use super::llm::Usage;

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

/// Controls how long a tool result's content stays in context.
///
/// Only the compaction system consumes this — other modules pass it through.
/// `CurrentRun` cleanup is keyed off `Message::User`. Tool-generated
/// interactions (e.g. ask_user responses) are `Message::ToolResult` and
/// do NOT trigger cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Retention {
    #[default]
    Normal,
    CurrentRun,
}

// ---------------------------------------------------------------------------
// Content types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ImageSource {
    Path { path: String },
    Base64 { data: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Content {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        #[serde(rename = "mimeType")]
        mime_type: String,
        source: ImageSource,
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

impl Content {
    /// Resolve image data: if `data` is empty but `source` is set, load from disk.
    /// Returns `(base64_data, mime_type)` or `None` if resolution fails.
    pub fn resolve_image_data(&self) -> Option<(String, String)> {
        match self {
            Content::Image { mime_type, source } => match source {
                ImageSource::Base64 { data } if !data.is_empty() => {
                    Some((data.clone(), mime_type.clone()))
                }
                ImageSource::Path { path } => match std::fs::read(path) {
                    Ok(bytes) => {
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        Some((b64, mime_type.clone()))
                    }
                    Err(_) => None,
                },
                ImageSource::Base64 { .. } => None,
            },
            _ => None,
        }
    }
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
        /// Unique completion identifier from the provider (e.g. `chatcmpl-xxx`, `msg_xxx`).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        response_id: Option<String>,
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
        #[serde(default)]
        retention: Retention,
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
            data: match serde_json::to_value(data) {
                Ok(v) => v,
                Err(_) => serde_json::Value::Null,
            },
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
