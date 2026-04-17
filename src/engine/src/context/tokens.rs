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

/// Estimate tokens for a single `Content` block.
fn single_content_tokens(c: &Content) -> usize {
    match c {
        Content::Text { text } => estimate_tokens(text),
        Content::Image { data, .. } => {
            let raw_bytes = data.len() * 3 / 4;
            (raw_bytes / 750).clamp(85, 16_000)
        }
        Content::Thinking { thinking, .. } => estimate_tokens(thinking),
        Content::ToolCall {
            name, arguments, ..
        } => estimate_tokens(name) + estimate_tokens(&arguments.to_string()) + 8,
    }
}

/// Compute pre-aggregated stats from LLM messages.
///
/// Image tokens are counted as a separate dimension (not included in
/// user/assistant/tool_result tokens), so:
///   total = user_tokens + assistant_tokens + tool_result_tokens + image_tokens
pub fn compute_call_stats(messages: &[Message]) -> LlmCallStats {
    compute_call_stats_iter(messages.iter())
}

/// Compute stats from `AgentMessage` slice (filters to LLM messages only).
pub fn compute_call_stats_from_agent_messages(messages: &[AgentMessage]) -> LlmCallStats {
    compute_call_stats_iter(messages.iter().filter_map(|m| m.as_llm()))
}

fn compute_call_stats_iter<'a>(messages: impl Iterator<Item = &'a Message>) -> LlmCallStats {
    let mut stats = LlmCallStats::default();

    for msg in messages {
        match msg {
            Message::User { content, .. } => {
                stats.user_count += 1;
                for c in content {
                    let tok = single_content_tokens(c);
                    if matches!(c, Content::Image { .. }) {
                        stats.image_count += 1;
                        stats.image_tokens += tok;
                    } else {
                        stats.user_tokens += tok;
                    }
                }
            }
            Message::Assistant { content, .. } => {
                stats.assistant_count += 1;
                for c in content {
                    let tok = single_content_tokens(c);
                    if matches!(c, Content::Image { .. }) {
                        stats.image_count += 1;
                        stats.image_tokens += tok;
                    } else {
                        stats.assistant_tokens += tok;
                    }
                }
            }
            Message::ToolResult {
                content, tool_name, ..
            } => {
                stats.tool_result_count += 1;
                let mut msg_tokens = 0usize;
                for c in content {
                    let tok = single_content_tokens(c);
                    if matches!(c, Content::Image { .. }) {
                        stats.image_count += 1;
                        stats.image_tokens += tok;
                    } else {
                        stats.tool_result_tokens += tok;
                        msg_tokens += tok;
                    }
                }
                stats.tool_details.push((tool_name.clone(), msg_tokens));
            }
        }
    }

    stats.tool_details.sort_by(|a, b| b.1.cmp(&a.1));
    stats
}
