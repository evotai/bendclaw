//! Clear expired tool results whose `Retention::CurrentRun` lifecycle has ended.
//!
//! **L0 — always-on**: runs unconditionally regardless of budget.
//!
//! `Retention::CurrentRun` with a subsequent `User` message → replace content
//! with `"[{tool_name} result cleared after use]"`.

use crate::context::compaction::phase::PhaseContext;
use crate::context::compaction::phase::PhaseResult;
use crate::context::compaction::CompactionAction;
use crate::context::compaction::CompactionMethod;
use crate::context::tokens::content_tokens;
use crate::types::*;

pub fn run(messages: Vec<AgentMessage>, _ctx: &PhaseContext) -> PhaseResult {
    // Pre-compute: for each index, is there a User message after it?
    let mut has_user_after = vec![false; messages.len()];
    let mut seen_user = false;
    for i in (0..messages.len()).rev() {
        has_user_after[i] = seen_user;
        if matches!(&messages[i], AgentMessage::Llm(Message::User { .. })) {
            seen_user = true;
        }
    }

    let mut actions = Vec::new();

    let result = messages
        .into_iter()
        .enumerate()
        .map(|(idx, msg)| {
            let should_clear = matches!(
                &msg,
                AgentMessage::Llm(Message::ToolResult {
                    retention: Retention::CurrentRun,
                    ..
                })
            ) && has_user_after[idx];

            if !should_clear {
                return msg;
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
                let before_tokens = content_tokens(&content);
                let replacement = vec![Content::Text {
                    text: format!("[{tool_name} result cleared after use]"),
                }];
                let after_tokens = content_tokens(&replacement);

                actions.push(CompactionAction {
                    index: idx,
                    tool_name: tool_name.clone(),
                    method: CompactionMethod::LifecycleReclaimed,
                    before_tokens,
                    after_tokens,
                    end_index: None,
                    related_count: None,
                });

                AgentMessage::Llm(Message::ToolResult {
                    tool_call_id,
                    tool_name,
                    content: replacement,
                    is_error,
                    timestamp,
                    retention,
                })
            } else {
                msg
            }
        })
        .collect();

    PhaseResult {
        messages: result,
        actions,
    }
}
