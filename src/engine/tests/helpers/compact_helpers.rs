use std::collections::HashSet;

use bendengine::context::*;
use bendengine::types::*;

/// Create a user message with controllable size.
pub fn sized_user(chars: usize) -> AgentMessage {
    let text = format!("user {}", "x".repeat(chars));
    AgentMessage::Llm(Message::user(&text))
}

/// Create an assistant text message.
pub fn assistant_text(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        stop_reason: StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
    })
}

/// Create a complete tool turn: assistant(tool_call) + tool_result.
/// Returns 2 messages with matching tool_call_id.
pub fn tool_turn(id: &str, tool_name: &str, output_chars: usize) -> Vec<AgentMessage> {
    vec![
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: id.into(),
                name: tool_name.into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: id.into(),
            tool_name: tool_name.into(),
            content: vec![Content::Text {
                text: "r".repeat(output_chars),
            }],
            is_error: false,
            timestamp: 0,
        }),
    ]
}

/// Flatten multiple message groups into a single Vec.
#[allow(dead_code)]
pub fn flatten(groups: Vec<Vec<AgentMessage>>) -> Vec<AgentMessage> {
    groups.into_iter().flatten().collect()
}

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
        "orphan detected: call_ids={:?}, result_ids={:?}",
        call_ids.difference(&result_ids).collect::<Vec<_>>(),
        result_ids.difference(&call_ids).collect::<Vec<_>>(),
    );
}

/// Assert all actions match the expected level's methods.
pub fn assert_actions_match_level(level: u8, actions: &[CompactionAction]) {
    for action in actions {
        match level {
            0 => panic!("level 0 should have no actions"),
            1 => assert!(
                action.method == CompactionMethod::Outline
                    || action.method == CompactionMethod::HeadTail,
                "level 1 action should be Outline or HeadTail, got {:?}",
                action.method
            ),
            2 => assert_eq!(
                action.method,
                CompactionMethod::Summarized,
                "level 2 action should be Summarized"
            ),
            3 => assert_eq!(
                action.method,
                CompactionMethod::Dropped,
                "level 3 action should be Dropped"
            ),
            _ => panic!("unexpected level {}", level),
        }
    }
}
