//! Token estimation — tiktoken-based for accuracy and consistency.
//!
//! Uses `o200k_base` (GPT-4o / o1 / o3 / GPT-5 family encoding) as the
//! universal tokenizer. This is a reasonable approximation for non-OpenAI
//! models and ensures all subsystems (compaction, call stats, context
//! tracking) agree on token counts.

use crate::provider::ToolDefinition;
use crate::types::*;

/// Conservative fixed token estimate for images.
/// Images are resized to 2000×2000 max before provider calls, and Claude's
/// visual token formula is roughly `width * height / 750`, so use the maximum
/// post-resize cost instead of a low placeholder.
const IMAGE_FIXED_TOKEN_ESTIMATE: usize = 5_333;

/// Estimate tokens for a text string using the tiktoken `o200k_base` encoding.
pub fn estimate_tokens(text: &str) -> usize {
    tiktoken_rs::o200k_base_singleton()
        .encode_with_special_tokens(text)
        .len()
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
            Content::Image { .. } => IMAGE_FIXED_TOKEN_ESTIMATE,
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
        Content::Image { .. } => IMAGE_FIXED_TOKEN_ESTIMATE,
        Content::Thinking { thinking, .. } => estimate_tokens(thinking),
        Content::ToolCall {
            name, arguments, ..
        } => estimate_tokens(name) + estimate_tokens(&arguments.to_string()) + 8,
    }
}

/// Estimate tokens for tool definitions.
pub fn tool_definition_tokens(tools: &[ToolDefinition]) -> usize {
    tools
        .iter()
        .map(|tool| match serde_json::to_string(tool) {
            Ok(json) => estimate_tokens(&json),
            Err(_) => estimate_tokens(&tool.name) + estimate_tokens(&tool.description),
        })
        .sum()
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
