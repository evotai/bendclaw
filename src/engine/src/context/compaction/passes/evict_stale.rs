//! Evict stale messages by dropping the middle of the conversation.
//!
//! **L2 — budget-gated**: only runs when over budget.
//!
//! Strategy: EvictionStrategy
//!   Keeps `keep_first` messages + `keep_recent` messages.
//!   Drops the middle, inserting a marker.
//!   If still over budget, uses priority-based retention:
//!     1. First `keep_first` messages (task goal) — always kept
//!     2. user/assistant messages (from tail)
//!     3. tool_result (lowest priority)

use crate::context::compaction::compact::CompactionAction;
use crate::context::compaction::compact::CompactionMethod;
use crate::context::compaction::pass::CompactContext;
use crate::context::compaction::pass::PassResult;
use crate::context::tokens::message_tokens;
use crate::context::tokens::total_tokens;
use crate::types::*;

pub fn run(messages: Vec<AgentMessage>, ctx: &CompactContext) -> PassResult {
    let len = messages.len();
    let first_end = ctx.keep_first.min(len);
    let recent_start = len.saturating_sub(ctx.keep_recent);

    if first_end >= recent_start {
        let result = keep_within_budget(&messages, first_end, ctx.compact_target);
        let dropped = len.saturating_sub(result.len());
        let actions = if dropped > 0 {
            vec![CompactionAction {
                index: 0,
                tool_name: "messages".into(),
                method: CompactionMethod::Dropped,
                before_tokens: total_tokens(&messages),
                after_tokens: total_tokens(&result),
                end_index: None,
                related_count: Some(dropped),
            }]
        } else {
            vec![]
        };
        return PassResult {
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
                method: CompactionMethod::Dropped,
                before_tokens: total_tokens(&messages),
                after_tokens: total_tokens(&result),
                end_index: None,
                related_count: Some(dropped),
            }]
        } else {
            vec![]
        };
        return PassResult {
            messages: result,
            actions,
        };
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

    PassResult {
        messages: result,
        actions,
    }
}

/// Keep messages within budget using priority-based retention.
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
        let tokens = message_tokens(msg);
        if tokens > remaining {
            continue;
        }
        remaining -= tokens;
        included[i] = true;
    }

    // Pass 2: tool results (low priority)
    for (i, msg) in rest.iter().enumerate().rev() {
        if included[i] {
            continue;
        }
        let tokens = message_tokens(msg);
        if tokens > remaining {
            continue;
        }
        remaining -= tokens;
        included[i] = true;
    }

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
