//! Shrink oversized messages using policy-driven truncation.
//!
//! **L0 — always-on cleanup**. Runs unconditionally; budget-gated tiers
//! (AgeEvict, NormalTrunc) only fire when over budget and stop as soon as
//! the running token count fits.
//!
//! Handles two message types:
//! - **User messages**: truncate oversized old (non-pinned, non-recent) user
//!   text when over budget; old images are stripped only under severe pressure.
//! - **Tool results**: policy-driven truncation via `ToolPolicy`.
//!   Images in old tool results follow the same severe-pressure stripping rule.
//!
//! Strategy: `ToolPolicy` from `policy::tool_policy()`.
//! Global thresholds control *when* a result is oversized; per-tool policy
//! controls *how* it is handled.

use super::truncate::*;
use super::user::*;
use crate::context::compaction::phase::PhaseContext;
use crate::context::compaction::phase::PhaseResult;
use crate::context::compaction::policy::tool_policy;
use crate::context::compaction::CompactionAction;
use crate::context::compaction::CompactionMethod;
use crate::context::tokens::content_tokens;
use crate::types::*;

pub fn run(messages: Vec<AgentMessage>, ctx: &PhaseContext, current_tokens: usize) -> PhaseResult {
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

            // Tier 4: ImageStrip — strip images only under severe pressure.
            let severely_over = running_tokens > ctx.budget + ctx.budget / 2;
            if over_budget && severely_over {
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

    PhaseResult {
        messages: result,
        actions,
    }
}
