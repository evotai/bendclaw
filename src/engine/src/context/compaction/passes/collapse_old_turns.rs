//! Collapse old assistant turns into short summaries.
//!
//! **L1 — budget-gated**: only runs when over budget, stops as soon as
//! the running token count fits within budget.
//!
//! Strategy: SummaryStrategy
//!   boundary = messages.len() - ctx.keep_recent
//!   For assistant turns before the boundary:
//!     1. Extract tool names from `Content::ToolCall`, deduplicate
//!     2. Extract up to 3 short text fragments (<= 200 chars each)
//!     3. Format: `[Summary] tool1, tool2 — "text1 text2"`
//!        (> 3 tools: `[Summary] [Assistant used N tool(s)]`)
//!     4. Only replace if summary is shorter than original
//!
//!   For user messages before the boundary (outside keep_first):
//!     Images are replaced with a text marker to reclaim tokens.

use crate::context::compaction::compact::CompactionAction;
use crate::context::compaction::compact::CompactionMethod;
use crate::context::compaction::pass::CompactContext;
use crate::context::compaction::pass::PassResult;
use crate::context::tokens::message_tokens;
use crate::types::*;

pub fn run(messages: Vec<AgentMessage>, ctx: &CompactContext, current_tokens: usize) -> PassResult {
    let len = messages.len();
    if len <= ctx.keep_recent {
        return PassResult {
            messages,
            actions: vec![],
        };
    }

    let boundary = len - ctx.keep_recent;
    let mut result = Vec::new();
    let mut actions = Vec::new();
    let mut running_tokens = current_tokens;

    let mut i = 0;
    while i < boundary {
        // Already fits within compact target — copy remaining pre-boundary messages as-is
        if running_tokens <= ctx.compact_target {
            while i < boundary {
                result.push(messages[i].clone());
                i += 1;
            }
            break;
        }

        let msg = &messages[i];
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                let turn_start = i;
                let before_tokens = message_tokens(msg);

                // Extract tool names (deduplicated, preserving order)
                let mut tool_names: Vec<String> = Vec::new();
                let mut seen_tools = std::collections::HashSet::new();
                for c in content {
                    if let Content::ToolCall { name, .. } = c {
                        if seen_tools.insert(name.clone()) {
                            tool_names.push(name.clone());
                        }
                    }
                }

                // Extract short text fragments (up to 3, <= 200 chars each)
                let text_parts: Vec<&str> = content
                    .iter()
                    .filter_map(|c| match c {
                        Content::Text { text } if text.len() <= 200 && !is_filler(text) => {
                            Some(text.as_str())
                        }
                        _ => None,
                    })
                    .take(3)
                    .collect();

                // Build summary
                let summary = if !tool_names.is_empty() {
                    let tools_part = if tool_names.len() <= 3 {
                        tool_names.join(", ")
                    } else {
                        format!("[Assistant used {} tool(s)]", tool_names.len())
                    };
                    if !text_parts.is_empty() {
                        format!("[Summary] {} — \"{}\"", tools_part, text_parts.join(" "))
                    } else {
                        format!("[Summary] {}", tools_part)
                    }
                } else if !text_parts.is_empty() {
                    format!("[Summary] {}", text_parts.join(" "))
                } else {
                    "[Summary] [Assistant response]".into()
                };

                let summary_msg = AgentMessage::Llm(Message::User {
                    content: vec![Content::Text { text: summary }],
                    timestamp: now_ms(),
                });
                let after_tokens = message_tokens(&summary_msg);

                // Peek ahead to count trailing tool results
                let mut peek = i + 1;
                let mut tool_result_count: usize = 0;
                let mut tool_result_tokens: usize = 0;
                while peek < boundary {
                    if let AgentMessage::Llm(Message::ToolResult { .. }) = &messages[peek] {
                        tool_result_tokens += message_tokens(&messages[peek]);
                        tool_result_count += 1;
                        peek += 1;
                    } else {
                        break;
                    }
                }

                let total_before = before_tokens + tool_result_tokens;

                // Only summarize if it actually saves tokens
                if after_tokens < total_before {
                    running_tokens -= total_before - after_tokens;
                    result.push(summary_msg);
                    i = peek;

                    actions.push(CompactionAction {
                        index: turn_start,
                        tool_name: "assistant".into(),
                        method: CompactionMethod::Summarized,
                        before_tokens: total_before,
                        after_tokens,
                        end_index: None,
                        related_count: Some(tool_result_count),
                    });
                } else {
                    // Keep original assistant + tool results as-is
                    result.push(msg.clone());
                    i += 1;
                    while i < boundary {
                        if let AgentMessage::Llm(Message::ToolResult { .. }) = &messages[i] {
                            result.push(messages[i].clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                }

                continue;
            }
            AgentMessage::Llm(Message::ToolResult { .. }) => {
                // Orphaned tool result (no preceding assistant) — skip
                i += 1;
                continue;
            }
            other => {
                // Strip images from old user messages to reclaim tokens
                // (but preserve keep_first pinned messages)
                if i >= ctx.keep_first {
                    if let AgentMessage::Llm(Message::User { content, timestamp }) = other {
                        let has_image = content.iter().any(|c| matches!(c, Content::Image { .. }));
                        if has_image {
                            let before_tokens = message_tokens(other);
                            let stripped: Vec<Content> = content
                                .iter()
                                .map(|c| match c {
                                    Content::Image { .. } => Content::Text {
                                        text: "[image omitted during compaction]".into(),
                                    },
                                    other => other.clone(),
                                })
                                .collect();
                            let replaced = AgentMessage::Llm(Message::User {
                                content: stripped,
                                timestamp: *timestamp,
                            });
                            let after_tokens = message_tokens(&replaced);
                            if after_tokens < before_tokens {
                                running_tokens -= before_tokens - after_tokens;
                                actions.push(CompactionAction {
                                    index: i,
                                    tool_name: "user".into(),
                                    method: CompactionMethod::OversizeCapped,
                                    before_tokens,
                                    after_tokens,
                                    end_index: None,
                                    related_count: None,
                                });
                                result.push(replaced);
                            } else {
                                result.push(other.clone());
                            }
                        } else {
                            result.push(other.clone());
                        }
                    } else {
                        result.push(other.clone());
                    }
                } else {
                    result.push(other.clone());
                }
            }
        }
        i += 1;
    }

    result.extend_from_slice(&messages[boundary..]);

    PassResult {
        messages: result,
        actions,
    }
}

/// Returns true for filler text that adds no value to a summary.
fn is_filler(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    matches!(
        t.as_str(),
        "done"
            | "done."
            | "ok"
            | "ok."
            | "sure"
            | "sure."
            | "i'll fix this"
            | "let me check"
            | "let me look"
    )
}
