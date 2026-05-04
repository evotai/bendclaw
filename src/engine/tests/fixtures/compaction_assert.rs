use std::collections::HashSet;

use evotengine::context::*;
use evotengine::types::*;

/// Assert no orphan tool_call / tool_result in a message list.
pub fn assert_no_orphan_tool_pairs(messages: &[AgentMessage]) {
    let mut call_ids = HashSet::new();
    let mut result_ids = HashSet::new();
    for msg in messages {
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                for c in content {
                    if let Content::ToolCall { id, .. } = c {
                        call_ids.insert(id.clone());
                    }
                }
            }
            AgentMessage::Llm(Message::ToolResult { tool_call_id, .. }) => {
                result_ids.insert(tool_call_id.clone());
            }
            _ => {}
        }
    }
    assert_eq!(
        call_ids,
        result_ids,
        "orphan detected: unmatched calls={:?}, unmatched results={:?}",
        call_ids.difference(&result_ids).collect::<Vec<_>>(),
        result_ids.difference(&call_ids).collect::<Vec<_>>(),
    );
}

/// Assert actions are consistent with the reported level.
///
/// The pipeline runs all passes in sequence, so a higher level can contain
/// actions from earlier passes. The `level` represents the *highest* pass
/// that produced actions:
///   0 = only LifecycleCleared (or no-op)
///   1 = collapse (Summarized), may also have cleanup/shrink actions
///   2 = evict (Dropped), may also have level-1 actions
pub fn assert_actions_match_level(level: u8, actions: &[CompactionAction]) {
    let allowed_at_level = |method: &CompactionMethod, lvl: u8| -> bool {
        match method {
            CompactionMethod::LifecycleReclaimed => true, // always allowed
            CompactionMethod::AgeCleared
            | CompactionMethod::ImageStripped
            | CompactionMethod::OversizeCapped
            | CompactionMethod::Outline
            | CompactionMethod::HeadTail => lvl >= 1,
            CompactionMethod::TurnCollapsed => lvl >= 2,
            CompactionMethod::MessagesEvicted => lvl >= 3,
        }
    };

    for action in actions {
        assert!(
            allowed_at_level(&action.method, level),
            "action {:?} not expected at level {}",
            action.method,
            level
        );
    }
}
