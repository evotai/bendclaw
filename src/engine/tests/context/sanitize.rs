use evotengine::context::*;
use evotengine::types::*;
use fixtures::compaction_assert::*;
use fixtures::message_dsl::*;

use super::fixtures;

// ---------------------------------------------------------------------------
// sanitize_tool_pairs tests
// ---------------------------------------------------------------------------

fn make_assistant_with_tool_call(tool_call_id: &str, tool_name: &str) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::ToolCall {
            id: tool_call_id.into(),
            name: tool_name.into(),
            arguments: serde_json::json!({}),
        }],
        stop_reason: StopReason::ToolUse,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn make_assistant_with_text_and_tool_call(
    text: &str,
    tool_call_id: &str,
    tool_name: &str,
) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text { text: text.into() }, Content::ToolCall {
            id: tool_call_id.into(),
            name: tool_name.into(),
            arguments: serde_json::json!({}),
        }],
        stop_reason: StopReason::ToolUse,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn make_tool_result(tool_call_id: &str, tool_name: &str) -> AgentMessage {
    AgentMessage::Llm(Message::ToolResult {
        tool_call_id: tool_call_id.into(),
        tool_name: tool_name.into(),
        content: vec![Content::Text { text: "ok".into() }],
        is_error: false,
        timestamp: 0,
        retention: Retention::Normal,
    })
}

#[test]
fn test_sanitize_orphan_tool_call() {
    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        make_assistant_with_tool_call("tc-1", "bash"),
        // no ToolResult for tc-1
    ];
    let result = sanitize_tool_pairs(messages);
    // assistant with only orphan tool_call should be removed entirely
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        AgentMessage::Llm(Message::User { .. })
    ));
}

#[test]
fn test_sanitize_orphan_tool_result() {
    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        // no assistant with tool_call for tc-1
        make_tool_result("tc-1", "bash"),
    ];
    let result = sanitize_tool_pairs(messages);
    // orphan tool_result should be removed
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        AgentMessage::Llm(Message::User { .. })
    ));
}

#[test]
fn test_sanitize_matched_pairs_intact() {
    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        make_assistant_with_tool_call("tc-1", "bash"),
        make_tool_result("tc-1", "bash"),
    ];
    let result = sanitize_tool_pairs(messages);
    assert_eq!(result.len(), 3);
}

#[test]
fn test_sanitize_mixed_content() {
    // assistant has text + orphan tool_call → only tool_call stripped, text preserved
    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        make_assistant_with_text_and_tool_call("I'll help", "tc-1", "bash"),
        // no ToolResult for tc-1
    ];
    let result = sanitize_tool_pairs(messages);
    assert_eq!(result.len(), 2);
    if let AgentMessage::Llm(Message::Assistant { content, .. }) = &result[1] {
        assert_eq!(content.len(), 1);
        assert!(matches!(&content[0], Content::Text { text } if text == "I'll help"));
    } else {
        panic!("expected assistant message");
    }
}

#[test]
fn test_sanitize_empty_assistant_removed() {
    // assistant only has orphan tool_call → entire message removed
    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        make_assistant_with_tool_call("tc-1", "bash"),
        // no ToolResult for tc-1
        AgentMessage::Llm(Message::user("next question")),
    ];
    let result = sanitize_tool_pairs(messages);
    assert_eq!(result.len(), 2);
    // both remaining should be user messages
    assert!(matches!(
        &result[0],
        AgentMessage::Llm(Message::User { .. })
    ));
    assert!(matches!(
        &result[1],
        AgentMessage::Llm(Message::User { .. })
    ));
}

// ---------------------------------------------------------------------------
// sanitize_tool_pairs DSL pattern tests
// ---------------------------------------------------------------------------

#[test]
fn test_sanitize_dsl_orphan_tool_removed() {
    // T = orphan tool call (no result) → removed by sanitize
    let messages = pat("u T u").build();
    let result = sanitize_tool_pairs(messages);
    // orphan T removed, two user messages remain
    assert_eq!(result.len(), 2);
    assert!(matches!(
        &result[0],
        AgentMessage::Llm(Message::User { .. })
    ));
    assert!(matches!(
        &result[1],
        AgentMessage::Llm(Message::User { .. })
    ));
}

#[test]
fn test_sanitize_dsl_paired_tools_intact() {
    // Paired tool calls stay intact
    let messages = pat("u tr u").build();
    let result = sanitize_tool_pairs(messages);
    assert_eq!(result.len(), 4); // u, t, r, u
}

#[test]
fn test_sanitize_dsl_mixed_paired_and_orphan() {
    // Paired + orphan: paired stays, orphan removed
    let messages = pat("u tr T u").build();
    let result = sanitize_tool_pairs(messages);
    // u, t, r survive; orphan T removed; trailing u stays
    assert_eq!(result.len(), 4); // u, t, r, u
}

#[test]
fn test_sanitize_dsl_many_orphans_between_users() {
    // Multiple orphan Ts between user messages
    let messages = pat("u T u T u T u").build();
    let result = sanitize_tool_pairs(messages);
    // All Ts removed, 4 user messages remain
    assert_eq!(result.len(), 4);
    for msg in &result {
        assert!(matches!(msg, AgentMessage::Llm(Message::User { .. })));
    }
}

#[test]
fn test_sanitize_dsl_orphan_after_valid_conversation() {
    // Normal conversation then orphan at end
    let messages = pat("u a u tr T u").build();
    let result = sanitize_tool_pairs(messages);
    // u, a, u, t, r survive; T removed; last u stays
    assert_eq!(result.len(), 6); // u, a, u, t, r, u
}

#[test]
fn test_compact_level2_no_orphans() {
    let messages = pat("u u u tr u").pad(800).build();

    let config = ContextConfig {
        max_context_tokens: 400,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 1,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert!(
        result.stats.level >= 2,
        "expected level >= 2, got {}",
        result.stats.level
    );
    assert_no_orphan_tool_pairs(&result.messages);
}

#[test]
fn test_compact_level3_no_orphans() {
    let messages = pat("u u tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr u")
        .pad(10)
        .build();

    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 50,
        keep_recent: 3,
        keep_first: 2,
        tool_output_max_lines: 20,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert_no_orphan_tool_pairs(&result.messages);
}
