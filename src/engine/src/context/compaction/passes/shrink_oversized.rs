//! Shrink oversized messages using policy-driven truncation.
//!
//! **L0 — always-on cleanup**. Runs unconditionally; budget-gated tiers
//! (AgeEvict, NormalTrunc) only fire when over budget and stop as soon as
//! the running token count fits.
//!
//! Handles two message types:
//! - **User messages**: truncate oversized old (non-pinned, non-recent) user
//!   text (budget-gated). Images are never stripped.
//! - **Tool results**: policy-driven truncation via `ToolPolicy`.
//!   Images in tool results are preserved.
//!
//! Strategy: `ToolPolicy` from `policy::tool_policy()`.
//! Global thresholds control *when* a result is oversized; per-tool policy
//! controls *how* it is handled.

use std::collections::HashMap;

use crate::context::compaction::compact::CompactionAction;
use crate::context::compaction::compact::CompactionMethod;
use crate::context::compaction::outline;
use crate::context::compaction::pass::CompactContext;
use crate::context::compaction::pass::PassResult;
use crate::context::compaction::policy::tool_policy;
use crate::context::tokens::content_tokens;
use crate::types::*;

pub fn run(messages: Vec<AgentMessage>, ctx: &CompactContext, current_tokens: usize) -> PassResult {
    let tool_call_index = build_tool_call_index(&messages);
    let len = messages.len();
    let recent_boundary = len.saturating_sub(ctx.keep_recent);

    let oversize_token_threshold = ctx
        .policy
        .oversize_abs_tokens
        .max((ctx.budget as f64 * ctx.policy.oversize_budget_ratio) as usize);

    let mut running_tokens = current_tokens;
    let mut actions = Vec::new();
    let mut result = Vec::with_capacity(len);

    for (idx, msg) in messages.into_iter().enumerate() {
        let is_recent = idx >= recent_boundary;
        let is_pinned = idx < ctx.keep_first;

        // --- User messages: oversized truncation + old image stripping ---
        if let AgentMessage::Llm(Message::User { content, timestamp }) = &msg {
            let action = classify_user_action(
                is_pinned,
                is_recent,
                running_tokens,
                ctx.budget,
                content,
                oversize_token_threshold,
            );
            match action {
                UserAction::TruncateOversized => {
                    let before_tokens = content_tokens(content);
                    let max_lines = ctx.tool_output_max_lines;

                    // Concatenate text blocks, truncate as a whole.
                    let mut combined_text = String::new();
                    for c in content {
                        if let Content::Text { text } = c {
                            if !combined_text.is_empty() {
                                combined_text.push('\n');
                            }
                            combined_text.push_str(text);
                        }
                    }
                    let truncated_text = truncate_text_head_tail(&combined_text, max_lines);
                    let truncated_text = crate::tools::validation::truncate_tool_text(
                        &truncated_text,
                        COMPACTION_MAX_BYTES,
                    );

                    // Preserve images alongside truncated text
                    let mut truncated: Vec<Content> = vec![Content::Text {
                        text: truncated_text,
                    }];
                    for c in content {
                        if let Content::Image { .. } = c {
                            truncated.push(c.clone());
                        }
                    }
                    let after_tokens = content_tokens(&truncated);
                    if after_tokens < before_tokens {
                        running_tokens -= before_tokens - after_tokens;
                        actions.push(CompactionAction {
                            index: idx,
                            tool_name: "user".into(),
                            method: CompactionMethod::OversizeCapped,
                            before_tokens,
                            after_tokens,
                            end_index: None,
                            related_count: None,
                        });
                    }
                    result.push(AgentMessage::Llm(Message::User {
                        content: truncated,
                        timestamp: *timestamp,
                    }));
                    continue;
                }
                UserAction::StripImages => {
                    let before_tokens = content_tokens(content);
                    let stripped: Vec<Content> = content
                        .iter()
                        .map(|c| match c {
                            Content::Image { .. } => Content::Text {
                                text: "[image]".into(),
                            },
                            other => other.clone(),
                        })
                        .collect();
                    let after_tokens = content_tokens(&stripped);
                    if after_tokens < before_tokens {
                        running_tokens -= before_tokens - after_tokens;
                        actions.push(CompactionAction {
                            index: idx,
                            tool_name: "user".into(),
                            method: CompactionMethod::AgeCleared,
                            before_tokens,
                            after_tokens,
                            end_index: None,
                            related_count: None,
                        });
                    }
                    result.push(AgentMessage::Llm(Message::User {
                        content: stripped,
                        timestamp: *timestamp,
                    }));
                    continue;
                }
                UserAction::Keep => {}
            }
        }

        let is_tool_result = matches!(&msg, AgentMessage::Llm(Message::ToolResult { .. }));
        if !is_tool_result {
            result.push(msg);
            continue;
        }

        if let AgentMessage::Llm(Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            timestamp,
            retention,
        }) = msg
        {
            let tokens = content_tokens(&content);
            let over_budget = running_tokens > ctx.budget;
            let tp = tool_policy(&tool_name);

            // Tier 1: OversizeCap — individually too large (always-on)
            if tokens > oversize_token_threshold {
                let max_lines = tp.oversize_max_lines.min(ctx.tool_output_max_lines);
                let truncated = truncate_content(
                    &content,
                    &tool_name,
                    &tool_call_id,
                    &tool_call_index,
                    max_lines,
                    tp.prefer_outline,
                );
                let after_tokens = content_tokens(&truncated);

                if after_tokens < tokens {
                    running_tokens -= tokens - after_tokens;
                    actions.push(CompactionAction {
                        index: idx,
                        tool_name: tool_name.clone(),
                        method: CompactionMethod::OversizeCapped,
                        before_tokens: tokens,
                        after_tokens,
                        end_index: None,
                        related_count: None,
                    });

                    result.push(AgentMessage::Llm(Message::ToolResult {
                        tool_call_id,
                        tool_name,
                        content: truncated,
                        is_error,
                        timestamp,
                        retention,
                    }));
                    continue;
                }
            }

            // Tier 2: AgeEvict — old result exceeding age threshold (budget-gated)
            if over_budget && !is_recent {
                if let Some(threshold) = tp.age_clear_threshold {
                    if tokens > threshold {
                        let marker = format!("[{tool_name} result cleared — {tokens} tokens]");
                        let replacement = vec![Content::Text { text: marker }];
                        let after_tokens = content_tokens(&replacement);

                        running_tokens -= tokens - after_tokens;
                        actions.push(CompactionAction {
                            index: idx,
                            tool_name: tool_name.clone(),
                            method: CompactionMethod::AgeCleared,
                            before_tokens: tokens,
                            after_tokens,
                            end_index: None,
                            related_count: None,
                        });

                        result.push(AgentMessage::Llm(Message::ToolResult {
                            tool_call_id,
                            tool_name,
                            content: replacement,
                            is_error,
                            timestamp,
                            retention,
                        }));
                        continue;
                    }
                }
            }

            // Tier 3: NormalTrunc — over budget, truncate tool results (budget-gated)
            if over_budget {
                let max_lines = tp.normal_max_lines.min(ctx.tool_output_max_lines);
                let truncated = truncate_content(
                    &content,
                    &tool_name,
                    &tool_call_id,
                    &tool_call_index,
                    max_lines,
                    tp.prefer_outline,
                );
                let after_tokens = content_tokens(&truncated);

                if after_tokens < tokens {
                    let method = detect_method(&content, &truncated);

                    running_tokens -= tokens - after_tokens;
                    actions.push(CompactionAction {
                        index: idx,
                        tool_name: tool_name.clone(),
                        method,
                        before_tokens: tokens,
                        after_tokens,
                        end_index: None,
                        related_count: None,
                    });

                    result.push(AgentMessage::Llm(Message::ToolResult {
                        tool_call_id,
                        tool_name,
                        content: truncated,
                        is_error,
                        timestamp,
                        retention,
                    }));
                    continue;
                }
            }

            // Tier 4: ImageStrip — strip images from tool results (budget-gated).
            // When severely over budget (>150%), also strip from recent messages.
            let severely_over = running_tokens > ctx.budget + ctx.budget / 2;
            if over_budget && (severely_over || !is_recent) {
                let has_images = content.iter().any(|c| matches!(c, Content::Image { .. }));
                if has_images {
                    let stripped: Vec<Content> = content
                        .iter()
                        .map(|c| match c {
                            Content::Image { .. } => Content::Text {
                                text: "[image]".into(),
                            },
                            other => other.clone(),
                        })
                        .collect();
                    let after_tokens = content_tokens(&stripped);
                    if after_tokens < tokens {
                        running_tokens -= tokens - after_tokens;
                        actions.push(CompactionAction {
                            index: idx,
                            tool_name: tool_name.clone(),
                            method: CompactionMethod::AgeCleared,
                            before_tokens: tokens,
                            after_tokens,
                            end_index: None,
                            related_count: None,
                        });

                        result.push(AgentMessage::Llm(Message::ToolResult {
                            tool_call_id,
                            tool_name,
                            content: stripped,
                            is_error,
                            timestamp,
                            retention,
                        }));
                        continue;
                    }
                }
            }

            // No truncation needed
            result.push(AgentMessage::Llm(Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                timestamp,
                retention,
            }));
        } else {
            result.push(msg);
        }
    }

    PassResult {
        messages: result,
        actions,
    }
}

// ---------------------------------------------------------------------------
// User message classification
// ---------------------------------------------------------------------------

/// What to do with a user message during shrink.
enum UserAction {
    /// Over budget + oversized: truncate text
    TruncateOversized,
    /// Over budget + has images: strip images
    StripImages,
    /// No action needed
    Keep,
}

/// Classify the action for a user message based on position and size.
fn classify_user_action(
    is_pinned: bool,
    is_recent: bool,
    running_tokens: usize,
    budget: usize,
    content: &[Content],
    oversize_threshold: usize,
) -> UserAction {
    if is_pinned {
        return UserAction::Keep;
    }
    // Severely over budget (>150%): strip images even from recent messages.
    // This is the last resort when images dominate context and normal
    // compaction (text-only) cannot bring it within budget.
    let severely_over = running_tokens > budget + budget / 2;
    if !is_recent {
        let tokens = content_tokens(content);
        if running_tokens > budget && tokens > oversize_threshold {
            return UserAction::TruncateOversized;
        }
    }
    if (severely_over || !is_recent)
        && running_tokens > budget
        && content.iter().any(|c| matches!(c, Content::Image { .. }))
    {
        return UserAction::StripImages;
    }
    UserAction::Keep
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an index from tool_call_id → ToolCall arguments.
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

/// Byte limit for tool result text after line truncation.
/// Catches cases where individual lines are very long (minified JSON/HTML).
const COMPACTION_MAX_BYTES: usize = 15_000;

/// Truncate content blocks using outline (if preferred) or head-tail,
/// then apply a byte cap to catch long single-line content.
fn truncate_content(
    content: &[Content],
    _tool_name: &str,
    tool_call_id: &str,
    tool_call_index: &HashMap<String, serde_json::Value>,
    max_lines: usize,
    prefer_outline: bool,
) -> Vec<Content> {
    content
        .iter()
        .map(|c| match c {
            Content::Text { text } => {
                let truncated = if prefer_outline {
                    try_outline_or_truncate(text, tool_call_index, tool_call_id, max_lines)
                } else {
                    truncate_text_head_tail(text, max_lines)
                };
                // Second pass: byte cap (line truncation alone may not be
                // enough when individual lines are very long).
                let truncated =
                    crate::tools::validation::truncate_tool_text(&truncated, COMPACTION_MAX_BYTES);
                Content::Text { text: truncated }
            }
            Content::Image { .. } => c.clone(),
            other => other.clone(),
        })
        .collect()
}

/// Try tree-sitter outline for read_file, fall back to head-tail.
fn try_outline_or_truncate(
    text: &str,
    tool_call_index: &HashMap<String, serde_json::Value>,
    tool_call_id: &str,
    max_lines: usize,
) -> String {
    // Extract file path and extension from the tool call arguments
    if let Some(args) = tool_call_index.get(tool_call_id) {
        if let Some(path_str) = args.get("path").and_then(|v| v.as_str()) {
            let ext = std::path::Path::new(path_str)
                .extension()
                .and_then(|e| e.to_str());
            if let Some(ext) = ext {
                if let Some(outlined) =
                    outline::extract_outline_from_read_file_output(text, ext, path_str)
                {
                    // Use outline only if it saves at least 10%
                    let threshold = text.len() / 10;
                    if outlined.len() + threshold < text.len() {
                        return outlined;
                    }
                }
            }
        }
    }

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

/// Detect whether outline or head-tail was used by checking content.
fn detect_method(original: &[Content], truncated: &[Content]) -> CompactionMethod {
    for t in truncated {
        if let Content::Text { text } = t {
            if text.contains("[Structural outline of") {
                return CompactionMethod::Outline;
            }
            if text.contains("[... ") && text.contains(" lines truncated ...]") {
                return CompactionMethod::HeadTail;
            }
        }
    }
    // If content changed but no marker detected, it was still capped
    let orig_len: usize = original
        .iter()
        .map(|c| match c {
            Content::Text { text } => text.len(),
            _ => 0,
        })
        .sum();
    let trunc_len: usize = truncated
        .iter()
        .map(|c| match c {
            Content::Text { text } => text.len(),
            _ => 0,
        })
        .sum();
    if trunc_len < orig_len {
        CompactionMethod::OversizeCapped
    } else {
        CompactionMethod::HeadTail
    }
}
