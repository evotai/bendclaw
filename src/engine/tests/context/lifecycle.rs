use evotengine::context::*;
use evotengine::types::*;

// ---------------------------------------------------------------------------
// Lifecycle cleanup (Level 0)
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_clears_current_run_after_user_message() {
    let messages = vec![
        AgentMessage::Llm(Message::user("analyze sql")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "c1".into(),
                name: "skill".into(),
                arguments: serde_json::json!({"skill_name": "db"}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "skill".into(),
            content: vec![Content::Text {
                text: "Long skill instructions here...".into(),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::CurrentRun,
        }),
        AgentMessage::Llm(Message::user("next question")),
    ];

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert_eq!(result.stats.level, 0);
    assert_eq!(result.stats.current_run_cleared, 1);

    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
        let text = match &content[0] {
            Content::Text { text } => text,
            _ => panic!("expected text"),
        };
        assert_eq!(text, "[skill result cleared after use]");
    } else {
        panic!("expected tool result at index 2");
    }
}

#[test]
fn lifecycle_preserves_current_run_without_user_after() {
    let messages = vec![
        AgentMessage::Llm(Message::user("analyze sql")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "c1".into(),
                name: "skill".into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "skill".into(),
            content: vec![Content::Text {
                text: "Skill instructions".into(),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::CurrentRun,
        }),
    ];

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert_eq!(result.stats.current_run_cleared, 0);

    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
        let text = match &content[0] {
            Content::Text { text } => text,
            _ => panic!("expected text"),
        };
        assert_eq!(text, "Skill instructions");
    } else {
        panic!("expected tool result at index 2");
    }
}

#[test]
fn lifecycle_does_not_affect_normal_retention() {
    let messages = vec![
        AgentMessage::Llm(Message::user("read file")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "c1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "read_file".into(),
            content: vec![Content::Text {
                text: "file content here".into(),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::user("next question")),
    ];

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert_eq!(result.stats.current_run_cleared, 0);

    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
        let text = match &content[0] {
            Content::Text { text } => text,
            _ => panic!("expected text"),
        };
        assert_eq!(text, "file content here");
    } else {
        panic!("expected tool result at index 2");
    }
}

#[test]
fn lifecycle_clears_multiple_current_run_results() {
    let messages = vec![
        AgentMessage::Llm(Message::user("q1")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "c1".into(),
                name: "skill".into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "skill".into(),
            content: vec![Content::Text {
                text: "skill 1".into(),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::CurrentRun,
        }),
        AgentMessage::Llm(Message::user("q2")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "c2".into(),
                name: "skill".into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c2".into(),
            tool_name: "skill".into(),
            content: vec![Content::Text {
                text: "skill 2".into(),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::CurrentRun,
        }),
        AgentMessage::Llm(Message::user("q3")),
    ];

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 20,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert_eq!(result.stats.current_run_cleared, 2);
}
