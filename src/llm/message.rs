use serde::Deserialize;
use serde::Serialize;

pub use crate::base::Content;
pub use crate::base::Role;
pub use crate::base::ToolCall;

/// Anthropic prompt caching control.
///
/// When set to `Ephemeral`, the API caches the content block across requests,
/// reducing input token costs for long conversations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CacheControl {
    Ephemeral,
}

/// A chat message sent to or received from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Vec<Content>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Anthropic cache_control — marks this message for prompt caching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl Default for ChatMessage {
    fn default() -> Self {
        Self {
            role: Role::User,
            content: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            cache_control: None,
        }
    }
}

impl ChatMessage {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: vec![Content::text(text)],
            ..Default::default()
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![Content::text(text)],
            ..Default::default()
        }
    }

    pub fn user_multimodal(parts: Vec<Content>) -> Self {
        Self {
            role: Role::User,
            content: parts,
            ..Default::default()
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![Content::text(text)],
            ..Default::default()
        }
    }

    pub fn assistant_with_tool_calls(text: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![Content::text(text)],
            tool_calls,
            ..Default::default()
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: vec![Content::text(text)],
            tool_call_id: Some(tool_call_id.into()),
            ..Default::default()
        }
    }

    pub fn with_cache_control(mut self) -> Self {
        self.cache_control = Some(CacheControl::Ephemeral);
        self
    }

    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}
