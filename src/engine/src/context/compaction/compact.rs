//! Tiered compaction — tool output truncation → turn summarization → drop middle.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

use super::outline;
use super::sanitize::sanitize_tool_pairs;
use crate::context::tokens::content_tokens;
use crate::context::tokens::total_tokens;
use crate::context::tracking::ContextConfig;
use crate::types::*;

// ---------------------------------------------------------------------------
// Compaction types
// ---------------------------------------------------------------------------

/// Per-tool token breakdown entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolTokenDetail {
    pub tool_name: String,
    pub tokens: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionStats {
    pub level: u8,
    pub before_message_count: usize,
    pub after_message_count: usize,
    pub before_estimated_tokens: usize,
    pub after_estimated_tokens: usize,
    pub tool_outputs_truncated: usize,
    pub turns_summarized: usize,
    pub messages_dropped: usize,
    /// Per-tool token breakdown before compaction (sorted by tokens desc).
    #[serde(default)]
    pub before_tool_details: Vec<ToolTokenDetail>,
    /// Per-tool token breakdown after compaction (sorted by tokens desc).
    #[serde(default)]
    pub after_tool_details: Vec<ToolTokenDetail>,
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub messages: Vec<AgentMessage>,
    pub stats: CompactionStats,
}

pub trait CompactionStrategy: Send + Sync {
    fn compact(&self, messages: Vec<AgentMessage>, config: &ContextConfig) -> CompactionResult;
}

pub struct DefaultCompaction;

impl CompactionStrategy for DefaultCompaction {
    fn compact(&self, messages: Vec<AgentMessage>, config: &ContextConfig) -> CompactionResult {
        compact_messages(messages, config)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect per-tool token details from messages, sorted by tokens descending.
fn collect_tool_details(messages: &[AgentMessage]) -> Vec<ToolTokenDetail> {
    let mut details = Vec::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::ToolResult {
            tool_name, content, ..
        }) = msg
        {
            details.push(ToolTokenDetail {
                tool_name: tool_name.clone(),
                tokens: content_tokens(content),
            });
        }
    }
    details.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    details
}

/// Build an index from tool_call_id → ToolCall arguments.
///
/// Used by Level 1 to look up the original call parameters (e.g. `path`
/// for `read_file`) when deciding how to truncate a `ToolResult`.
fn build_tool_call_index(messages: &[AgentMessage]) -> HashMap<String, serde_json::Value> {
    let mut index = HashMap::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for c in content {
                if let Content::ToolCall { id, arguments, .. } = c {
                    index.insert(id.clone(), arguments.clone());
                }
            }
        }
    }
    index
}

// ---------------------------------------------------------------------------
// Tiered compaction
// ---------------------------------------------------------------------------

/// Compact messages to fit within the token budget using tiered strategy.
///
/// - Level 1: Truncate tool outputs (keep head + tail, or outline for code)
/// - Level 2: Summarize old turns (replace details with one-liner)
/// - Level 3: Drop old messages (keep first + recent only)
///
/// Each level is tried in order. Returns as soon as messages fit.
pub fn compact_messages(messages: Vec<AgentMessage>, config: &ContextConfig) -> CompactionResult {
    let budget = config
        .max_context_tokens
        .saturating_sub(config.system_prompt_tokens);

    let before_message_count = messages.len();
    let before_estimated_tokens = total_tokens(&messages);
    let before_tool_details = collect_tool_details(&messages);

    let make_result = |msgs: Vec<AgentMessage>, level: u8, stats: CompactionStats| {
        let msgs = sanitize_tool_pairs(msgs);
        let after_message_count = msgs.len();
        let after_estimated_tokens = total_tokens(&msgs);
        let after_tool_details = collect_tool_details(&msgs);
        CompactionResult {
            messages: msgs,
            stats: CompactionStats {
                level,
                before_message_count,
                after_message_count,
                before_estimated_tokens,
                after_estimated_tokens,
                before_tool_details: before_tool_details.clone(),
                after_tool_details,
                ..stats
            },
        }
    };

    if before_estimated_tokens <= budget {
        return make_result(messages, 0, CompactionStats::default());
    }

    let (compacted, tool_outputs_truncated) =
        level1_truncate_tool_outputs(&messages, config.tool_output_max_lines);
    if total_tokens(&compacted) <= budget {
        return make_result(compacted, 1, CompactionStats {
            tool_outputs_truncated,
            ..Default::default()
        });
    }

    let (compacted, turns_summarized) = level2_summarize_old_turns(&compacted, config.keep_recent);
    if total_tokens(&compacted) <= budget {
        return make_result(compacted, 2, CompactionStats {
            tool_outputs_truncated,
            turns_summarized,
            ..Default::default()
        });
    }

    let (compacted, messages_dropped) = level3_drop_middle(&compacted, config, budget);
    make_result(compacted, 3, CompactionStats {
        tool_outputs_truncated,
        turns_summarized,
        messages_dropped,
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Level 1: Truncate tool outputs
// ---------------------------------------------------------------------------

/// Level 1: Truncate long tool outputs.
///
/// For `read_file` results containing source code, attempts structural outline
/// extraction via tree-sitter. For all other tools (or when outline fails /
/// produces longer output), falls back to head+tail truncation.
pub fn level1_truncate_tool_outputs(
    messages: &[AgentMessage],
    max_lines: usize,
) -> (Vec<AgentMessage>, usize) {
    let tool_call_index = build_tool_call_index(messages);
    let mut truncated_count = 0;
    let result = messages
        .iter()
        .map(|msg| match msg {
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                timestamp,
            }) => {
                let mut was_truncated = false;
                let truncated_content: Vec<Content> = content
                    .iter()
                    .map(|c| match c {
                        Content::Text { text } => {
                            let result = if tool_name == "read_file" {
                                try_outline_or_truncate(
                                    text,
                                    &tool_call_index,
                                    tool_call_id,
                                    max_lines,
                                )
                            } else {
                                truncate_text_head_tail(text, max_lines)
                            };
                            if result.len() < text.len() {
                                was_truncated = true;
                            }
                            Content::Text { text: result }
                        }
                        other => other.clone(),
                    })
                    .collect();
                if was_truncated {
                    truncated_count += 1;
                }
                AgentMessage::Llm(Message::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content: truncated_content,
                    is_error: *is_error,
                    timestamp: *timestamp,
                })
            }
            other => other.clone(),
        })
        .collect();
    (result, truncated_count)
}

/// Try tree-sitter outline for a `read_file` result, fall back to head+tail.
fn try_outline_or_truncate(
    text: &str,
    tool_call_index: &HashMap<String, serde_json::Value>,
    tool_call_id: &str,
    max_lines: usize,
) -> String {
    // Only attempt outline if the text is long enough to warrant it
    if text.lines().count() <= max_lines {
        return text.to_string();
    }

    // Look up the file path from the ToolCall arguments
    let path = tool_call_index
        .get(tool_call_id)
        .and_then(|args| args.get("path"))
        .and_then(|v| v.as_str());

    let path = match path {
        Some(p) => p,
        None => return truncate_text_head_tail(text, max_lines),
    };

    // Extract extension
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str());

    let ext = match ext {
        Some(e) => e,
        None => return truncate_text_head_tail(text, max_lines),
    };

    // Parse the read_file numbered output and try outline
    match outline::extract_outline_from_read_file_output(text, ext, path) {
        Some(outlined) if outlined.len() < text.len() => outlined,
        _ => truncate_text_head_tail(text, max_lines),
    }
}

/// Truncate text keeping first N/2 and last N/2 lines.
pub fn truncate_text_head_tail(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }

    let head = max_lines / 2;
    let tail = max_lines - head;
    let omitted = lines.len() - head - tail;

    let mut result = lines[..head].join("\n");
    result.push_str(&format!("\n\n[... {} lines truncated ...]\n\n", omitted));
    result.push_str(&lines[lines.len() - tail..].join("\n"));
    result
}

// ---------------------------------------------------------------------------
// Level 2: Summarize old turns
// ---------------------------------------------------------------------------

/// Level 2: Summarize old assistant turns.
///
/// Keeps the last `keep_recent` messages in full detail.
/// For older messages: assistant messages with tool calls get replaced
/// with a short summary, and their tool results get dropped.
fn level2_summarize_old_turns(
    messages: &[AgentMessage],
    keep_recent: usize,
) -> (Vec<AgentMessage>, usize) {
    let len = messages.len();
    if len <= keep_recent {
        return (messages.to_vec(), 0);
    }

    let boundary = len - keep_recent;
    let mut result = Vec::new();
    let mut turns_summarized = 0;

    let mut i = 0;
    while i < boundary {
        let msg = &messages[i];
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                let text_parts: Vec<&str> = content
                    .iter()
                    .filter_map(|c| match c {
                        Content::Text { text } => {
                            if text.len() > 200 {
                                None
                            } else {
                                Some(text.as_str())
                            }
                        }
                        _ => None,
                    })
                    .collect();

                let tool_count = content
                    .iter()
                    .filter(|c| matches!(c, Content::ToolCall { .. }))
                    .count();

                let summary = if !text_parts.is_empty() {
                    text_parts.join(" ")
                } else if tool_count > 0 {
                    format!("[Assistant used {} tool(s)]", tool_count)
                } else {
                    "[Assistant response]".into()
                };

                result.push(AgentMessage::Llm(Message::User {
                    content: vec![Content::Text {
                        text: format!("[Summary] {}", summary),
                    }],
                    timestamp: now_ms(),
                }));
                turns_summarized += 1;

                i += 1;
                while i < boundary {
                    if let AgentMessage::Llm(Message::ToolResult { .. }) = &messages[i] {
                        i += 1;
                    } else {
                        break;
                    }
                }
                continue;
            }
            AgentMessage::Llm(Message::ToolResult { .. }) => {
                i += 1;
                continue;
            }
            other => {
                result.push(other.clone());
            }
        }
        i += 1;
    }

    result.extend_from_slice(&messages[boundary..]);
    (result, turns_summarized)
}

// ---------------------------------------------------------------------------
// Level 3: Drop middle
// ---------------------------------------------------------------------------

/// Level 3: Drop middle messages, keeping first + recent.
fn level3_drop_middle(
    messages: &[AgentMessage],
    config: &ContextConfig,
    budget: usize,
) -> (Vec<AgentMessage>, usize) {
    let len = messages.len();
    let first_end = config.keep_first.min(len);
    let recent_start = len.saturating_sub(config.keep_recent);

    if first_end >= recent_start {
        let result = keep_within_budget(messages, budget);
        let dropped = len.saturating_sub(result.len());
        return (result, dropped);
    }

    let first_msgs = &messages[..first_end];
    let recent_msgs = &messages[recent_start..];
    let removed = recent_start - first_end;

    let marker = AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: format!(
                "[Context compacted: {} messages removed to fit context window]",
                removed
            ),
        }],
        timestamp: now_ms(),
    });

    let mut result = first_msgs.to_vec();
    result.push(marker);
    result.extend_from_slice(recent_msgs);

    if total_tokens(&result) > budget {
        let result = keep_within_budget(&result, budget);
        let dropped = len.saturating_sub(result.len());
        return (result, dropped);
    }

    (result, removed)
}

/// Keep as many recent messages as fit within budget.
fn keep_within_budget(messages: &[AgentMessage], budget: usize) -> Vec<AgentMessage> {
    let mut result = Vec::new();
    let mut remaining = budget;

    for msg in messages.iter().rev() {
        let tokens = crate::context::tokens::message_tokens(msg);
        if tokens > remaining {
            break;
        }
        remaining -= tokens;
        result.push(msg.clone());
    }

    result.reverse();

    if result.len() < messages.len() {
        let removed = messages.len() - result.len();
        result.insert(
            0,
            AgentMessage::Llm(Message::User {
                content: vec![Content::Text {
                    text: format!("[Context compacted: {} messages removed]", removed),
                }],
                timestamp: now_ms(),
            }),
        );
    }

    result
}
