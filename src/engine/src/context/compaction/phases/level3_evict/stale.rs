//! Evict stale messages by dropping the upper-middle of the conversation.
//!
//! Runs for hard token pressure or when `max_messages` is exceeded. This is the
//! primary long-session control: keep the beginning and recent tail, then drop
//! old middle content instead of endlessly summarizing it.
//!
//! Strategy:
//!   Keeps `keep_first` messages + `keep_recent` messages.
//!   Drops the middle, inserting a marker.
//!   If still over budget, uses tail-first retention:
//!     Protect `keep_first`, then fill remaining budget from tail.

use crate::context::compaction::compact::CompactionAction;
use crate::context::compaction::compact::CompactionMethod;
use crate::context::compaction::phase::PhaseContext;
use crate::context::compaction::phase::PhaseResult;
use crate::context::tokens::message_tokens;
use crate::context::tokens::total_tokens;
use crate::types::*;

pub fn run(messages: Vec<AgentMessage>, ctx: &PhaseContext) -> PhaseResult {
    let len = messages.len();
    let target_messages = ctx
        .keep_first
        .saturating_add(ctx.keep_recent)
        .saturating_add(1)
        .max(1);
    if len <= target_messages && total_tokens(&messages) <= ctx.compact_target {
        return PhaseResult {
            messages,
            actions: Vec::new(),
        };
    }

    let first_end = ctx.keep_first.min(len);
    let recent_start = len.saturating_sub(ctx.keep_recent);

    if first_end >= recent_start {
        let result = keep_within_budget(&messages, first_end, ctx.compact_target);
        let dropped = len.saturating_sub(result.len());
        let actions = if dropped > 0 {
            vec![CompactionAction {
                index: 0,
                tool_name: "messages".into(),
                method: CompactionMethod::MessagesEvicted,
                before_tokens: total_tokens(&messages),
                after_tokens: total_tokens(&result),
                end_index: None,
                related_count: Some(dropped),
            }]
        } else {
            vec![]
        };
        return PhaseResult {
            messages: result,
            actions,
        };
    }

    let first_msgs = &messages[..first_end];
    let recent_msgs = &messages[recent_start..];
    let removed = recent_start - first_end;

    let dropped_tokens: usize = messages[first_end..recent_start]
        .iter()
        .map(message_tokens)
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
    let marker_tokens = message_tokens(&marker);

    let mut result = first_msgs.to_vec();
    result.push(marker);
    result.extend_from_slice(recent_msgs);

    if total_tokens(&result) > ctx.compact_target {
        let result = keep_within_budget(&result, first_end, ctx.compact_target);
        let dropped = len.saturating_sub(result.len());
        let actions = if dropped > 0 {
            vec![CompactionAction {
                index: 0,
                tool_name: "messages".into(),
                method: CompactionMethod::MessagesEvicted,
                before_tokens: total_tokens(&messages),
                after_tokens: total_tokens(&result),
                end_index: None,
                related_count: Some(dropped),
            }]
        } else {
            vec![]
        };
        return PhaseResult {
            messages: result,
            actions,
        };
    }

    let actions = vec![CompactionAction {
        index: first_end,
        tool_name: "messages".into(),
        method: CompactionMethod::MessagesEvicted,
        before_tokens: dropped_tokens.max(marker_tokens),
        after_tokens: marker_tokens,
        end_index: Some(recent_start.saturating_sub(1)),
        related_count: Some(removed),
    }];

    PhaseResult {
        messages: result,
        actions,
    }
}

/// Keep messages within budget using tail-first retention.
///
/// Protects the first `keep_first` messages, then fills the remaining
/// budget from the tail (most-recent-first). Old messages — including
/// compaction summaries accumulated at the front — are naturally dropped.
fn keep_within_budget(
    messages: &[AgentMessage],
    keep_first: usize,
    budget: usize,
) -> Vec<AgentMessage> {
    if messages.is_empty() {
        return Vec::new();
    }

    let protected_end = keep_first.max(1).min(messages.len());
    let protected = &messages[..protected_end];
    let protected_tokens: usize = protected.iter().map(message_tokens).sum();

    if protected_tokens >= budget {
        let first = messages[0].clone();
        let first_tokens = message_tokens(&first);
        if first_tokens > budget {
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

    // Tail-first: most recent messages first. Old summaries naturally
    // fall behind newer messages and are dropped when budget fills.
    let mut tail: Vec<AgentMessage> = Vec::new();
    for msg in rest.iter().rev() {
        let tokens = message_tokens(msg);
        if tokens > remaining {
            break;
        }
        remaining -= tokens;
        tail.push(msg.clone());
    }
    tail.reverse();

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
