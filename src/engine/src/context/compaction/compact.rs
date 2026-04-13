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

/// Byte limit for tool result text during compaction (Level 1).
/// More aggressive than the execution-time limit since we're actively
/// trying to reclaim space.
const COMPACTION_MAX_BYTES: usize = 15_000;

// ---------------------------------------------------------------------------
// Compaction types
// ---------------------------------------------------------------------------

/// Per-tool token breakdown entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolTokenDetail {
    pub tool_name: String,
    pub tokens: usize,
}

/// Describes what happened to a single item during compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionAction {
    /// Message index in the original list (0-based).
    pub index: usize,
    /// Tool name (Level 1), "assistant" (Level 2), or "messages" (Level 3).
    pub tool_name: String,
    /// What method was used.
    pub method: CompactionMethod,
    /// Tokens before compaction.
    pub before_tokens: usize,
    /// Tokens after compaction.
    pub after_tokens: usize,
    /// End index for range actions (Level 3 drop).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<usize>,
    /// Count of related messages (e.g. tool results in a Level 2 turn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_count: Option<usize>,
}

/// The method used to compact a tool result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompactionMethod {
    /// Tree-sitter structural outline extraction
    Outline,
    /// Head + tail truncation
    HeadTail,
    /// Skipped (content was short enough)
    Skipped,
    /// Turn summarized (Level 2)
    Summarized,
    /// Dropped (Level 3)
    Dropped,
    /// CurrentRun result cleared after run completed
    LifecycleCleared,
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
    pub current_run_cleared: usize,
    /// Per-tool token breakdown before compaction (sorted by tokens desc).
    #[serde(default)]
    pub before_tool_details: Vec<ToolTokenDetail>,
    /// Per-tool token breakdown after compaction (sorted by tokens desc).
    #[serde(default)]
    pub after_tool_details: Vec<ToolTokenDetail>,
    /// Per-message compaction actions (what happened to each tool result).
    #[serde(default)]
    pub actions: Vec<CompactionAction>,
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

    // Level 0: lifecycle cleanup (unconditional)
    let (messages, current_run_cleared, lifecycle_actions) = compact_run_once(messages);

    if total_tokens(&messages) <= budget {
        return make_result(messages, 0, CompactionStats {
            current_run_cleared,
            actions: lifecycle_actions,
            ..Default::default()
        });
    }

    let (compacted, tool_outputs_truncated, l1_actions) =
        level1_truncate_tool_outputs(&messages, config.tool_output_max_lines);
    if total_tokens(&compacted) <= budget {
        let mut actions = lifecycle_actions.clone();
        actions.extend(l1_actions);
        return make_result(compacted, 1, CompactionStats {
            current_run_cleared,
            tool_outputs_truncated,
            actions,
            ..Default::default()
        });
    }

    let (compacted, turns_summarized, l2_actions) =
        level2_summarize_old_turns(&compacted, config.keep_recent);
    if total_tokens(&compacted) <= budget {
        let mut actions = lifecycle_actions.clone();
        actions.extend(l2_actions);
        return make_result(compacted, 2, CompactionStats {
            current_run_cleared,
            tool_outputs_truncated,
            turns_summarized,
            actions,
            ..Default::default()
        });
    }

    let (compacted, messages_dropped, l3_actions) = level3_drop_middle(&compacted, config, budget);
    let mut actions = lifecycle_actions;
    actions.extend(l3_actions);
    make_result(compacted, 3, CompactionStats {
        current_run_cleared,
        tool_outputs_truncated,
        turns_summarized,
        messages_dropped,
        actions,
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Level 0: Lifecycle cleanup
// ---------------------------------------------------------------------------

fn compact_run_once(
    messages: Vec<AgentMessage>,
) -> (Vec<AgentMessage>, usize, Vec<CompactionAction>) {
    let mut has_user_after = vec![false; messages.len()];
    let mut seen_user = false;
    for i in (0..messages.len()).rev() {
        has_user_after[i] = seen_user;
        if matches!(&messages[i], AgentMessage::Llm(Message::User { .. })) {
            seen_user = true;
        }
    }

    let mut cleared = 0;
    let mut actions = Vec::new();

    let result = messages
        .into_iter()
        .enumerate()
        .map(|(idx, msg)| match msg {
            AgentMessage::Llm(Message::ToolResult {
                retention: Retention::CurrentRun,
                ref tool_name,
                ref content,
                ..
            }) if has_user_after[idx] => {
                let before_tokens = content_tokens(content);
                let replacement = vec![Content::Text {
                    text: format!("[{tool_name} output consumed]"),
                }];
                let after_tokens = content_tokens(&replacement);

                cleared += 1;
                actions.push(CompactionAction {
                    index: idx,
                    tool_name: tool_name.clone(),
                    method: CompactionMethod::LifecycleCleared,
                    before_tokens,
                    after_tokens,
                    end_index: None,
                    related_count: None,
                });

                if let AgentMessage::Llm(Message::ToolResult {
                    tool_call_id,
                    tool_name,
                    is_error,
                    timestamp,
                    retention,
                    ..
                }) = msg
                {
                    AgentMessage::Llm(Message::ToolResult {
                        tool_call_id,
                        tool_name,
                        content: replacement,
                        is_error,
                        timestamp,
                        retention,
                    })
                } else {
                    unreachable!()
                }
            }
            other => other,
        })
        .collect();

    (result, cleared, actions)
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
) -> (Vec<AgentMessage>, usize, Vec<CompactionAction>) {
    let tool_call_index = build_tool_call_index(messages);
    let mut truncated_count = 0;
    let mut actions = Vec::new();
    let result = messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| match msg {
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                timestamp,
                retention,
            }) => {
                let before_tokens = content_tokens(content);
                let mut method = CompactionMethod::Skipped;
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
                            // Second pass: byte cap (line truncation alone may
                            // not be enough when individual lines are very long).
                            let result = crate::tools::validation::truncate_tool_text(
                                &result,
                                COMPACTION_MAX_BYTES,
                            );
                            if result.len() < text.len() {
                                method = if result.contains("Structural outline") {
                                    CompactionMethod::Outline
                                } else {
                                    CompactionMethod::HeadTail
                                };
                            }
                            Content::Text { text: result }
                        }
                        other => other.clone(),
                    })
                    .collect();
                let after_tokens = content_tokens(&truncated_content);
                if method != CompactionMethod::Skipped {
                    truncated_count += 1;
                    actions.push(CompactionAction {
                        index: idx,
                        tool_name: tool_name.clone(),
                        method,
                        before_tokens,
                        after_tokens,
                        end_index: None,
                        related_count: None,
                    });
                }
                AgentMessage::Llm(Message::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content: truncated_content,
                    is_error: *is_error,
                    timestamp: *timestamp,
                    retention: *retention,
                })
            }
            other => other.clone(),
        })
        .collect();
    (result, truncated_count, actions)
}

/// Try tree-sitter outline for a `read_file` result, fall back to head+tail.
///
/// Always attempts outline first (regardless of line count). Uses outline only
/// if it saves at least 10% of the original size. Falls back to head+tail
/// truncation otherwise.
fn try_outline_or_truncate(
    text: &str,
    tool_call_index: &HashMap<String, serde_json::Value>,
    tool_call_id: &str,
    max_lines: usize,
) -> String {
    // Look up the file path from the ToolCall arguments
    let path = tool_call_index
        .get(tool_call_id)
        .and_then(|args| args.get("path"))
        .and_then(|v| v.as_str());

    if let Some(path) = path {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str());

        if let Some(ext) = ext {
            if let Some(outlined) = outline::extract_outline_from_read_file_output(text, ext, path)
            {
                // Use outline only if it saves at least 10%
                let threshold = text.len() / 10;
                if outlined.len() + threshold < text.len() {
                    return outlined;
                }
            }
        }
    }

    // Outline not available or not enough savings — fall back to head+tail
    truncate_text_head_tail(text, max_lines)
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
) -> (Vec<AgentMessage>, usize, Vec<CompactionAction>) {
    let len = messages.len();
    if len <= keep_recent {
        return (messages.to_vec(), 0, Vec::new());
    }

    let boundary = len - keep_recent;
    let mut result = Vec::new();
    let mut turns_summarized = 0;
    let mut actions = Vec::new();

    let mut i = 0;
    while i < boundary {
        let msg = &messages[i];
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                let turn_start = i;
                let before_tokens = crate::context::tokens::message_tokens(msg);

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

                let summary_msg = AgentMessage::Llm(Message::User {
                    content: vec![Content::Text {
                        text: format!("[Summary] {}", summary),
                    }],
                    timestamp: now_ms(),
                });
                let after_tokens = crate::context::tokens::message_tokens(&summary_msg);

                // Peek ahead to count trailing tool results
                let mut peek = i + 1;
                let mut tool_result_count: usize = 0;
                let mut tool_result_tokens: usize = 0;
                while peek < boundary {
                    if let AgentMessage::Llm(Message::ToolResult { .. }) = &messages[peek] {
                        tool_result_tokens +=
                            crate::context::tokens::message_tokens(&messages[peek]);
                        tool_result_count += 1;
                        peek += 1;
                    } else {
                        break;
                    }
                }

                let total_before = before_tokens + tool_result_tokens;

                // Only summarize if it actually saves tokens
                if after_tokens < total_before {
                    result.push(summary_msg);
                    turns_summarized += 1;
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
    (result, turns_summarized, actions)
}

// ---------------------------------------------------------------------------
// Level 3: Drop middle
// ---------------------------------------------------------------------------

/// Level 3: Drop middle messages, keeping first + recent.
fn level3_drop_middle(
    messages: &[AgentMessage],
    config: &ContextConfig,
    budget: usize,
) -> (Vec<AgentMessage>, usize, Vec<CompactionAction>) {
    let len = messages.len();
    let first_end = config.keep_first.min(len);
    let recent_start = len.saturating_sub(config.keep_recent);

    if first_end >= recent_start {
        let result = keep_within_budget(messages, first_end, budget);
        let dropped = len.saturating_sub(result.len());
        let actions = if dropped > 0 {
            vec![CompactionAction {
                index: 0,
                tool_name: "messages".into(),
                method: CompactionMethod::Dropped,
                before_tokens: total_tokens(messages),
                after_tokens: total_tokens(&result),
                end_index: None,
                related_count: Some(dropped),
            }]
        } else {
            Vec::new()
        };
        return (result, dropped, actions);
    }

    let first_msgs = &messages[..first_end];
    let recent_msgs = &messages[recent_start..];
    let removed = recent_start - first_end;

    // Calculate tokens of the dropped range
    let dropped_tokens: usize = messages[first_end..recent_start]
        .iter()
        .map(crate::context::tokens::message_tokens)
        .sum();

    let marker = AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: format!(
                "[Context compacted: {} messages removed to fit context window]",
                removed
            ),
        }],
        timestamp: now_ms(),
    });
    let marker_tokens = crate::context::tokens::message_tokens(&marker);

    let mut result = first_msgs.to_vec();
    result.push(marker);
    result.extend_from_slice(recent_msgs);

    if total_tokens(&result) > budget {
        let result = keep_within_budget(&result, first_end, budget);
        let dropped = len.saturating_sub(result.len());
        let actions = if dropped > 0 {
            vec![CompactionAction {
                index: 0,
                tool_name: "messages".into(),
                method: CompactionMethod::Dropped,
                before_tokens: total_tokens(messages),
                after_tokens: total_tokens(&result),
                end_index: None,
                related_count: Some(dropped),
            }]
        } else {
            Vec::new()
        };
        return (result, dropped, actions);
    }

    let actions = vec![CompactionAction {
        index: first_end,
        tool_name: "messages".into(),
        method: CompactionMethod::Dropped,
        before_tokens: dropped_tokens,
        after_tokens: marker_tokens,
        end_index: Some(recent_start.saturating_sub(1)),
        related_count: Some(removed),
    }];

    (result, removed, actions)
}

/// Keep messages within budget using priority-based retention.
///
/// Priority order:
/// 1. First `keep_first` messages (task goal / context) — always kept
/// 2. Recent messages (from tail)
/// 3. Older user messages preferred over tool results
///
/// Tool results are the first to be dropped since they are the largest
/// and least critical for maintaining task context.
fn keep_within_budget(
    messages: &[AgentMessage],
    keep_first: usize,
    budget: usize,
) -> Vec<AgentMessage> {
    if messages.is_empty() {
        return Vec::new();
    }

    // Always reserve the first `keep_first` messages (typically the user's
    // task goal and early context).
    let protected_end = keep_first.max(1).min(messages.len());
    let protected = &messages[..protected_end];
    let protected_tokens: usize = protected
        .iter()
        .map(crate::context::tokens::message_tokens)
        .sum();

    if protected_tokens >= budget {
        // Protected prefix alone exceeds budget — degrade to first message
        // only, and truncate it if even that exceeds budget.
        let first = messages[0].clone();
        let first_tokens = crate::context::tokens::message_tokens(&first);
        if first_tokens > budget {
            // Truncate the first message's text content to fit.
            if let AgentMessage::Llm(Message::User { content, timestamp }) = first {
                let capped = crate::tools::validation::cap_tool_result_content(content, budget * 4);
                return vec![AgentMessage::Llm(Message::User {
                    content: capped,
                    timestamp,
                })];
            }
        }
        return vec![first];
    }

    let mut remaining = budget - protected_tokens;
    let rest = &messages[protected_end..];

    // Fill from the tail, but prioritize user messages over tool results.
    // Two passes: first pass picks user + assistant messages, second pass
    // fills remaining budget with tool results.

    // Pass 1: user and assistant messages (high priority)
    let mut included = vec![false; rest.len()];
    for (i, msg) in rest.iter().enumerate().rev() {
        let is_user_or_assistant = matches!(
            msg,
            AgentMessage::Llm(Message::User { .. }) | AgentMessage::Llm(Message::Assistant { .. })
        );
        if !is_user_or_assistant {
            continue;
        }
        let tokens = crate::context::tokens::message_tokens(msg);
        if tokens > remaining {
            continue;
        }
        remaining -= tokens;
        included[i] = true;
    }

    // Pass 2: tool results (low priority, fill remaining budget)
    for (i, msg) in rest.iter().enumerate().rev() {
        if included[i] {
            continue;
        }
        let tokens = crate::context::tokens::message_tokens(msg);
        if tokens > remaining {
            continue;
        }
        remaining -= tokens;
        included[i] = true;
    }

    // Collect in order
    let mut tail: Vec<AgentMessage> = Vec::new();
    for (i, msg) in rest.iter().enumerate() {
        if included[i] {
            tail.push(msg.clone());
        }
    }

    let kept = protected_end + tail.len();
    let removed = messages.len() - kept;

    let mut result: Vec<AgentMessage> = protected.to_vec();
    if removed > 0 {
        result.push(AgentMessage::Llm(Message::User {
            content: vec![Content::Text {
                text: format!("[Context compacted: {} messages removed]", removed),
            }],
            timestamp: now_ms(),
        }));
    }
    result.extend(tail);
    result
}
