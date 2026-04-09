//! Token estimation — fast, no external deps.

use crate::types::*;

/// Rough token estimate: ~4 chars per token for English text.
/// Good enough for context budgeting. Use tiktoken-rs for precision.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Estimate tokens for a single message
pub fn message_tokens(msg: &AgentMessage) -> usize {
    match msg {
        AgentMessage::Llm(m) => match m {
            Message::User { content, .. } => content_tokens(content) + 4,
            Message::Assistant { content, .. } => content_tokens(content) + 4,
            Message::ToolResult {
                content, tool_name, ..
            } => content_tokens(content) + estimate_tokens(tool_name) + 8,
        },
        AgentMessage::Extension(ext) => estimate_tokens(&ext.data.to_string()) + 4,
    }
}

pub fn content_tokens(content: &[Content]) -> usize {
    content
        .iter()
        .map(|c| match c {
            Content::Text { text } => estimate_tokens(text),
            Content::Image { data, .. } => {
                // Estimate tokens from base64 data length:
                // base64 len * 3/4 = raw bytes; ~750 bytes per token for images.
                // Floor at 85 (Anthropic minimum), cap at 16000.
                let raw_bytes = data.len() * 3 / 4;
                (raw_bytes / 750).clamp(85, 16_000)
            }
            Content::Thinking { thinking, .. } => estimate_tokens(thinking),
            Content::ToolCall {
                name, arguments, ..
            } => estimate_tokens(name) + estimate_tokens(&arguments.to_string()) + 8,
        })
        .sum()
}

/// Estimate total tokens for a message list
pub fn total_tokens(messages: &[AgentMessage]) -> usize {
    messages.iter().map(message_tokens).sum()
}
