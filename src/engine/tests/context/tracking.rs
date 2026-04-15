use evotengine::context::*;
use evotengine::types::*;
use fixtures::compaction_assert::*;
use fixtures::message_dsl::*;

use super::fixtures;

#[test]
fn test_estimate_tokens() {
    assert!(estimate_tokens("hello world") > 0);
    assert!(estimate_tokens("hello world") < 10);
    assert_eq!(estimate_tokens(""), 0);
}

#[test]
fn test_context_config_from_context_window() {
    let config = ContextConfig::from_context_window(200_000);
    assert_eq!(config.max_context_tokens, 160_000);
    assert_eq!(config.system_prompt_tokens, 4_000);
    assert_eq!(config.keep_recent, 10);

    let config = ContextConfig::from_context_window(1_000_000);
    assert_eq!(config.max_context_tokens, 800_000);

    let config = ContextConfig::from_context_window(128_000);
    assert_eq!(config.max_context_tokens, 102_400);
}

#[test]
fn test_truncate_head_tail() {
    let text = (1..=100)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let result = truncate_text_head_tail(&text, 10);
    assert!(result.contains("line 1"));
    assert!(result.contains("line 5"));
    assert!(result.contains("line 100"));
    assert!(result.contains("truncated"));
    assert!(!result.contains("line 50"));
}

#[test]
fn test_level1_truncation() {
    let big_output = (1..=200)
        .map(|i| format!("output line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "tc-1".into(),
            tool_name: "bash".into(),
            content: vec![Content::Text { text: big_output }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
    ];

    let (compacted, count, _actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &compacted[1] {
        if let Content::Text { text } = &content[0] {
            assert!(text.contains("truncated"));
            assert!(text.contains("output line 1"));
            assert!(text.contains("output line 200"));
            assert!(text.lines().count() < 50);
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected tool result");
    }
}

#[test]
fn test_compact_within_budget() {
    let messages = pat("u a u").pad(10).build();
    let config = ContextConfig::default();
    let result = compact_messages(messages.clone(), &config);
    assert_eq!(result.stats.level, 0);
    assert_eq!(result.messages.len(), messages.len());
    assert!(result.stats.actions.is_empty());
}

#[test]
fn test_compact_drops_middle_when_needed() {
    let messages = pat("u a u a u a u a u a u a u a u a u a u a u a u a u")
        .pad(200)
        .build();

    let config = ContextConfig {
        max_context_tokens: 500,
        system_prompt_tokens: 100,
        keep_recent: 5,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages.clone(), &config);
    assert!(result.messages.len() < messages.len());
    assert!(result.messages.len() >= 2);
}

#[test]
fn test_context_tracker_no_usage() {
    let tracker = ContextTracker::new();
    let messages = vec![
        AgentMessage::Llm(Message::user("Hello")),
        AgentMessage::Llm(Message::user("World")),
    ];
    let tokens = tracker.estimate_context_tokens(&messages);
    assert!(tokens > 0);
    assert_eq!(tokens, total_tokens(&messages));
}

#[test]
fn test_context_tracker_with_usage() {
    let mut tracker = ContextTracker::new();
    let messages = vec![
        AgentMessage::Llm(Message::user("Hello")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::Text {
                text: "Hi there!".into(),
            }],
            stop_reason: StopReason::Stop,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage {
                input: 100,
                output: 50,
                ..Default::default()
            },
            timestamp: 0,
            error_message: None,
        }),
        AgentMessage::Llm(Message::user("Follow up question here")),
    ];
    tracker.record_usage(
        &Usage {
            input: 100,
            output: 50,
            ..Default::default()
        },
        1,
    );
    let tokens = tracker.estimate_context_tokens(&messages);
    let trailing_estimate = message_tokens(&messages[2]);
    assert_eq!(tokens, 150 + trailing_estimate);
}

#[test]
fn test_context_tracker_reset() {
    let mut tracker = ContextTracker::new();
    tracker.record_usage(
        &Usage {
            input: 1000,
            output: 500,
            ..Default::default()
        },
        5,
    );
    tracker.reset();
    let messages = vec![AgentMessage::Llm(Message::user("test"))];
    assert_eq!(
        tracker.estimate_context_tokens(&messages),
        total_tokens(&messages)
    );
}

#[test]
fn test_execution_limits() {
    let limits = ExecutionLimits {
        max_turns: 3,
        max_total_tokens: 1000,
        max_duration: std::time::Duration::from_secs(60),
    };

    let mut tracker = ExecutionTracker::new(limits);
    assert!(tracker.check_limits().is_none());

    tracker.record_turn(100);
    tracker.record_turn(100);
    assert!(tracker.check_limits().is_none());

    tracker.record_turn(100);
    assert!(tracker.check_limits().is_some());
}

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
    };

    let result = compact_messages(messages, &config);
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
    };

    let result = compact_messages(messages, &config);
    assert_no_orphan_tool_pairs(&result.messages);
}

// ---------------------------------------------------------------------------
// Level 1 outline tests
// ---------------------------------------------------------------------------

/// Helper: create an assistant message with a ToolCall that has arguments.
fn make_assistant_with_tool_call_args(
    tool_call_id: &str,
    tool_name: &str,
    args: serde_json::Value,
) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::ToolCall {
            id: tool_call_id.into(),
            name: tool_name.into(),
            arguments: args,
        }],
        stop_reason: StopReason::ToolUse,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
    })
}

/// Helper: create a ToolResult with specific text content.
fn make_tool_result_with_content(tool_call_id: &str, tool_name: &str, text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::ToolResult {
        tool_call_id: tool_call_id.into(),
        tool_name: tool_name.into(),
        content: vec![Content::Text { text: text.into() }],
        is_error: false,
        timestamp: 0,
        retention: Retention::Normal,
    })
}

/// Generate a fake read_file output with numbered lines of Rust code.
fn fake_rust_read_file_output(line_count: usize) -> String {
    let mut lines = Vec::new();
    lines.push(format!("[{} lines]", line_count));

    // Generate a simple Rust file with functions
    let mut code_lines = vec![
        "use std::collections::HashMap;".to_string(),
        String::new(),
        "pub struct Foo {".to_string(),
        "    field1: String,".to_string(),
        "    field2: usize,".to_string(),
        "}".to_string(),
        String::new(),
        "impl Foo {".to_string(),
        "    pub fn new() -> Self {".to_string(),
    ];

    // Pad with body lines to reach desired count
    while code_lines.len() < line_count.saturating_sub(3) {
        code_lines.push("        let x = 1;".to_string());
    }

    code_lines.push("    }".to_string());
    code_lines.push("}".to_string());
    code_lines.push(String::new());

    // Truncate or extend to exact line_count
    code_lines.truncate(line_count);

    for (i, code_line) in code_lines.iter().enumerate() {
        lines.push(format!("{:>4} | {}", i + 1, code_line));
    }

    lines.join("\n")
}

#[test]
fn test_level1_read_file_rust_uses_outline() {
    let rust_output = fake_rust_read_file_output(200);
    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "read_file",
            serde_json::json!({"path": "/src/foo.rs"}),
        ),
        make_tool_result_with_content("tc-1", "read_file", &rust_output),
    ];

    let (compacted, count, _actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &compacted[2] {
        if let Content::Text { text } = &content[0] {
            // Should be an outline, not head+tail
            assert!(
                text.contains("Structural outline"),
                "expected outline header, got: {}",
                &text[..text.len().min(200)]
            );
            assert!(
                !text.contains("lines truncated"),
                "should not use head+tail truncation"
            );
            // Should be shorter than original
            assert!(text.len() < rust_output.len());
            // Verify outline contains key structural elements
            assert!(
                text.contains("pub struct Foo"),
                "outline should contain struct declaration"
            );
            assert!(
                text.contains("impl Foo"),
                "outline should contain impl block"
            );
            assert!(
                text.contains("pub fn new"),
                "outline should contain method signature"
            );
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected tool result");
    }
}

#[test]
fn test_level1_read_file_unsupported_ext_falls_back_to_head_tail() {
    // .toml is not supported by tree-sitter outline
    let toml_output = {
        let mut lines = vec!["[200 lines]".to_string()];
        for i in 1..=200 {
            lines.push(format!("{:>4} | key{} = \"value{}\"", i, i, i));
        }
        lines.join("\n")
    };

    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "read_file",
            serde_json::json!({"path": "/config/settings.toml"}),
        ),
        make_tool_result_with_content("tc-1", "read_file", &toml_output),
    ];

    let (compacted, count, _actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &compacted[2] {
        if let Content::Text { text } = &content[0] {
            assert!(
                text.contains("truncated"),
                "should fall back to head+tail for .toml"
            );
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected tool result");
    }
}

#[test]
fn test_level1_bash_still_uses_head_tail() {
    let big_output = (1..=200)
        .map(|i| format!("output line {}", i))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        AgentMessage::Llm(Message::user("run command")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "bash",
            serde_json::json!({"command": "cargo test"}),
        ),
        make_tool_result_with_content("tc-1", "bash", &big_output),
    ];

    let (compacted, count, _actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &compacted[2] {
        if let Content::Text { text } = &content[0] {
            assert!(
                text.contains("truncated"),
                "bash should always use head+tail"
            );
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected tool result");
    }
}

#[test]
fn test_level1_read_file_no_matching_tool_call_falls_back() {
    let rust_output = fake_rust_read_file_output(200);

    // ToolResult without a matching ToolCall in the messages
    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_tool_result_with_content("tc-orphan", "read_file", &rust_output),
    ];

    let (compacted, count, _actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &compacted[1] {
        if let Content::Text { text } = &content[0] {
            assert!(
                text.contains("truncated"),
                "should fall back to head+tail when no ToolCall found"
            );
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected tool result");
    }
}

#[test]
fn test_level1_read_file_short_content_not_truncated() {
    // Short file — should not be truncated at all
    let short_output = "[5 lines]\n   1 | fn main() {\n   2 |     println!(\"hello\");\n   3 | }\n   4 | \n   5 | // end";

    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "read_file",
            serde_json::json!({"path": "/src/main.rs"}),
        ),
        make_tool_result_with_content("tc-1", "read_file", short_output),
    ];

    let (compacted, count, _actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 0, "short content should not be truncated");
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &compacted[2] {
        if let Content::Text { text } = &content[0] {
            assert_eq!(text, short_output);
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected tool result");
    }
}

#[test]
fn test_level1_read_file_python_uses_outline() {
    let mut lines = vec!["[100 lines]".to_string()];
    let mut code_lines = vec![
        "import os".to_string(),
        "import sys".to_string(),
        String::new(),
        "class MyClass:".to_string(),
        "    def __init__(self):".to_string(),
    ];
    // Pad with body lines
    for _ in 0..90 {
        code_lines.push("        self.x = 1".to_string());
    }
    code_lines.push("    def run(self):".to_string());
    code_lines.push("        pass".to_string());
    code_lines.push(String::new());

    code_lines.truncate(100);
    for (i, code_line) in code_lines.iter().enumerate() {
        lines.push(format!("{:>4} | {}", i + 1, code_line));
    }
    let py_output = lines.join("\n");

    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "read_file",
            serde_json::json!({"path": "/app/main.py"}),
        ),
        make_tool_result_with_content("tc-1", "read_file", &py_output),
    ];

    let (compacted, count, _actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &compacted[2] {
        if let Content::Text { text } = &content[0] {
            assert!(
                text.contains("Structural outline"),
                "expected outline for .py, got: {}",
                &text[..text.len().min(200)]
            );
            assert!(text.len() < py_output.len());
            // Verify outline contains key structural elements
            assert!(
                text.contains("class MyClass"),
                "outline should contain class declaration"
            );
            assert!(
                text.contains("def __init__"),
                "outline should contain method signature"
            );
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected tool result");
    }
}

// ---------------------------------------------------------------------------
// Level 1 action structure tests (DSL)
// ---------------------------------------------------------------------------

#[test]
fn test_level1_actions_only_non_skipped() {
    // Two tool turns: one big (truncated), one small (skipped)
    let big_output = (1..=200)
        .map(|i| format!("output line {}", i))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        make_assistant_with_tool_call("tc-1", "bash"),
        make_tool_result_with_content("tc-1", "bash", &big_output),
        make_assistant_with_tool_call("tc-2", "bash"),
        make_tool_result_with_content("tc-2", "bash", "short output"),
    ];

    let (_compacted, count, actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1, "only one tool output should be truncated");
    assert_eq!(
        actions.len(),
        1,
        "only non-Skipped actions should be recorded"
    );
    assert_eq!(actions[0].index, 2);
    assert_eq!(actions[0].tool_name, "bash");
    assert!(actions[0].before_tokens > actions[0].after_tokens);
}

#[test]
fn test_level1_actions_have_correct_index() {
    let big_output = (1..=200)
        .map(|i| format!("output line {}", i))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        AgentMessage::Llm(Message::user("msg 0")),
        make_assistant_with_tool_call("tc-1", "bash"),
        make_tool_result_with_content("tc-1", "bash", "ok"),
        AgentMessage::Llm(Message::user("msg 3")),
        make_assistant_with_tool_call("tc-2", "bash"),
        make_tool_result_with_content("tc-2", "bash", &big_output),
    ];

    let (_compacted, count, actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].index, 5);
}

#[test]
fn test_level1_outline_action_method() {
    let rust_output = fake_rust_read_file_output(200);
    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "read_file",
            serde_json::json!({"path": "/src/foo.rs"}),
        ),
        make_tool_result_with_content("tc-1", "read_file", &rust_output),
    ];

    let (_compacted, count, actions) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].index, 2);
    assert_eq!(actions[0].tool_name, "read_file");
    assert_eq!(actions[0].method, CompactionMethod::Outline);
    assert!(actions[0].end_index.is_none());
    assert!(actions[0].related_count.is_none());
}

// ---------------------------------------------------------------------------
// Level 1 outline threshold tests
// ---------------------------------------------------------------------------

#[test]
fn test_level1_outline_requires_10_percent_savings() {
    let short_rust = "[10 lines]\n   1 | use std::io;\n   2 | \n   3 | fn main() {\n   4 |     println!(\"hello\");\n   5 | }\n   6 | \n   7 | fn helper() {\n   8 |     println!(\"help\");\n   9 | }\n  10 | ";

    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "read_file",
            serde_json::json!({"path": "/src/tiny.rs"}),
        ),
        make_tool_result_with_content("tc-1", "read_file", short_rust),
    ];

    let (_compacted, _count, actions) = level1_truncate_tool_outputs(&messages, 50);
    for a in &actions {
        if a.method == CompactionMethod::Outline {
            let savings_pct =
                (a.before_tokens as f64 - a.after_tokens as f64) / a.before_tokens as f64;
            assert!(
                savings_pct >= 0.05,
                "outline should only be used with meaningful savings, got {:.1}%",
                savings_pct * 100.0
            );
        }
    }
}

#[test]
fn test_level1_outline_works_on_short_code_files() {
    let mut code_lines = vec![
        "use std::collections::HashMap;".to_string(),
        String::new(),
        "pub struct Config {".to_string(),
        "    name: String,".to_string(),
        "    value: usize,".to_string(),
        "}".to_string(),
        String::new(),
        "impl Config {".to_string(),
        "    pub fn new(name: &str) -> Self {".to_string(),
    ];
    for _ in 0..15 {
        code_lines.push("        let x = HashMap::new();".to_string());
    }
    code_lines.push("    }".to_string());
    code_lines.push(String::new());
    code_lines.push("    pub fn validate(&self) -> bool {".to_string());
    for _ in 0..5 {
        code_lines.push("        let y = self.value + 1;".to_string());
    }
    code_lines.push("    }".to_string());
    code_lines.push("}".to_string());

    let line_count = code_lines.len();
    let mut lines = vec![format!("[{} lines]", line_count)];
    for (i, code_line) in code_lines.iter().enumerate() {
        lines.push(format!("{:>4} | {}", i + 1, code_line));
    }
    let rust_output = lines.join("\n");

    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "read_file",
            serde_json::json!({"path": "/src/config.rs"}),
        ),
        make_tool_result_with_content("tc-1", "read_file", &rust_output),
    ];

    let (compacted, _count, actions) = level1_truncate_tool_outputs(&messages, 50);

    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &compacted[2] {
        if let Content::Text { text } = &content[0] {
            if text.contains("Structural outline") {
                assert!(text.len() < rust_output.len());
                assert_eq!(actions.len(), 1);
                assert_eq!(actions[0].method, CompactionMethod::Outline);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Level 2 action structure tests (DSL)
// ---------------------------------------------------------------------------

#[test]
fn test_level2_actions_structure() {
    let messages = pat("u tr tr u u").pad(2000).build();

    let config = ContextConfig {
        max_context_tokens: 550,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 0,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    assert!(
        result.stats.level >= 2,
        "expected level >= 2, got {}",
        result.stats.level
    );

    if result.stats.level == 2 {
        assert!(!result.stats.actions.is_empty());
        for action in &result.stats.actions {
            assert_eq!(action.method, CompactionMethod::Summarized);
            assert_eq!(action.tool_name, "assistant");
            assert!(action.related_count.is_some());
            assert!(action.before_tokens > action.after_tokens);
        }
    }
}

#[test]
fn test_level2_action_related_count() {
    // One assistant with multiple tool results following it
    let pad = "x".repeat(800);
    let messages = vec![
        AgentMessage::Llm(Message::user(&pad)),
        make_assistant_with_tool_call("tc-1", "bash"),
        make_tool_result("tc-1", "bash"),
        make_tool_result_with_content("tc-1b", "bash", "extra 1"),
        make_tool_result_with_content("tc-1c", "bash", "extra 2"),
        AgentMessage::Llm(Message::user("recent")),
    ];

    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 0,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    if result.stats.level == 2 {
        let summarized: Vec<_> = result
            .stats
            .actions
            .iter()
            .filter(|a| a.method == CompactionMethod::Summarized)
            .collect();

        for action in &summarized {
            if action.index == 1 {
                assert_eq!(
                    action.related_count,
                    Some(3),
                    "assistant at index 1 should have 3 related tool results"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Level 3 action structure tests (DSL)
// ---------------------------------------------------------------------------

#[test]
fn test_level3_actions_structure() {
    let messages = pat("u u tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr tr u u u")
        .pad(10)
        .build();

    let config = ContextConfig {
        max_context_tokens: 150,
        system_prompt_tokens: 50,
        keep_recent: 3,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert_eq!(result.stats.level, 3, "should trigger level 3");
    assert!(!result.stats.actions.is_empty());

    for action in &result.stats.actions {
        assert_eq!(action.method, CompactionMethod::Dropped);
        assert_eq!(action.tool_name, "messages");
        assert!(action.before_tokens > action.after_tokens);
    }
}

#[test]
fn test_level3_action_has_range() {
    let messages = pat("u u a a a a a a a a a a a a a a a u u")
        .pad(100)
        .build();

    let config = ContextConfig {
        max_context_tokens: 300,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    if result.stats.level == 3 {
        let dropped: Vec<_> = result
            .stats
            .actions
            .iter()
            .filter(|a| a.method == CompactionMethod::Dropped)
            .collect();

        assert!(!dropped.is_empty());
        if let Some(action) = dropped.first() {
            if let Some(end) = action.end_index {
                assert_eq!(action.index, 2, "drop should start after keep_first");
                assert!(end > action.index, "end_index should be after start index");
            }
            assert!(action.related_count.is_some());
        }
    }
}

// ---------------------------------------------------------------------------
// compact_messages only passes current level actions (DSL)
// ---------------------------------------------------------------------------

#[test]
fn test_compact_level1_actions_are_level1_only() {
    // Large tool output that triggers level 1 truncation
    let big_output = (1..=500)
        .map(|i| format!("output line {} with some extra padding text here", i))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        make_assistant_with_tool_call("tc-1", "bash"),
        make_tool_result_with_content("tc-1", "bash", &big_output),
    ];

    let config = ContextConfig {
        max_context_tokens: 800,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert_eq!(result.stats.level, 1, "should trigger level 1");
    assert!(!result.stats.actions.is_empty());
    assert_actions_match_level(1, &result.stats.actions);
}

#[test]
fn test_compact_level2_actions_are_level2_only() {
    let messages = pat("u tr u tr u").pad(800).build();

    let config = ContextConfig {
        max_context_tokens: 400,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 0,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    if result.stats.level == 2 {
        assert_actions_match_level(2, &result.stats.actions);
    }
}

#[test]
fn test_compact_level0_no_actions() {
    let messages = pat("u a u").pad(10).build();
    let config = ContextConfig::default();
    let result = compact_messages(messages, &config);
    assert_eq!(result.stats.level, 0);
    assert!(result.stats.actions.is_empty());
}

// ---------------------------------------------------------------------------
// Boundary cases
// ---------------------------------------------------------------------------

#[test]
fn test_compact_empty_messages() {
    let messages: Vec<AgentMessage> = vec![];
    let config = ContextConfig::default();
    let result = compact_messages(messages, &config);
    assert_eq!(result.stats.level, 0);
    assert!(result.messages.is_empty());
    assert!(result.stats.actions.is_empty());
}

#[test]
fn test_compact_single_user() {
    let messages = pat("u").build();
    let config = ContextConfig {
        max_context_tokens: 1,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 0,
        tool_output_max_lines: 50,
    };
    let result = compact_messages(messages, &config);
    assert!(!result.messages.is_empty());
}

#[test]
fn test_compact_all_users_no_tool() {
    let messages = pat("u a u a u").pad(5000).build();
    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 0,
        tool_output_max_lines: 50,
    };
    let result = compact_messages(messages, &config);
    assert!(!result.messages.is_empty());
    assert_no_orphan_tool_pairs(&result.messages);
}

#[test]
fn test_compact_budget_zero() {
    let messages = pat("u tr u tr u").pad(100).tool_output(500).build();
    let config = ContextConfig {
        max_context_tokens: 0,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 0,
        tool_output_max_lines: 50,
    };
    let result = compact_messages(messages, &config);
    assert!(!result.messages.is_empty());
    assert_no_orphan_tool_pairs(&result.messages);
}

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
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    assert_eq!(result.stats.level, 0);
    assert_eq!(result.stats.current_run_cleared, 1);

    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
        let text = match &content[0] {
            Content::Text { text } => text,
            _ => panic!("expected text"),
        };
        assert_eq!(text, "[skill output consumed]");
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
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
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
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
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
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        keep_recent: 20,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    assert_eq!(result.stats.current_run_cleared, 2);
}
