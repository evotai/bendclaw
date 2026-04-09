use bendengine::context::*;
use bendengine::types::*;

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
        }),
    ];

    let (compacted, count) = level1_truncate_tool_outputs(&messages, 20);
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
    let messages = vec![
        AgentMessage::Llm(Message::user("Hello")),
        AgentMessage::Llm(Message::user("World")),
    ];
    let config = ContextConfig::default();
    let result = compact_messages(messages, &config);
    assert_eq!(result.messages.len(), 2);
}

#[test]
fn test_compact_drops_middle_when_needed() {
    let mut messages = Vec::new();
    for i in 0..100 {
        messages.push(AgentMessage::Llm(Message::user(format!(
            "Message {} {}",
            i,
            "x".repeat(200)
        ))));
    }

    let config = ContextConfig {
        max_context_tokens: 500,
        system_prompt_tokens: 100,
        keep_recent: 5,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert!(result.messages.len() < 100);
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

/// Helper: assert no orphan tool_call / tool_result in a message list.
fn assert_no_orphan_tool_pairs(messages: &[AgentMessage]) {
    let mut call_ids = std::collections::HashSet::new();
    let mut result_ids = std::collections::HashSet::new();
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
        call_ids, result_ids,
        "tool_call ids and tool_result ids must match"
    );
}

#[test]
fn test_compact_level2_no_orphans() {
    // Regression: level2_summarize_old_turns can split an assistant(tool_calls)
    // from its ToolResults when the boundary falls between them.
    //
    // Layout (6 messages, keep_recent=2 → boundary=4):
    //   [0] user (long padding)
    //   [1] user (long padding)
    //   [2] user (long padding)
    //   [3] assistant(tool_call tc-split)   ← old zone, will be summarized
    //   [4] tool_result(tc-split)           ← recent zone, kept as-is → orphan!
    //   [5] user
    //
    // Budget is set so level1 output still exceeds it but level2 fits.

    let pad = "x".repeat(800); // ~200 tokens each
    let messages = vec![
        AgentMessage::Llm(Message::user(&pad)),
        AgentMessage::Llm(Message::user(&pad)),
        AgentMessage::Llm(Message::user(&pad)),
        make_assistant_with_tool_call("tc-split", "bash"),
        make_tool_result("tc-split", "bash"),
        AgentMessage::Llm(Message::user("final")),
    ];

    let config = ContextConfig {
        // Total before compaction: ~3*204 + ~16 + ~14 + ~5 = ~647 tokens
        // Budget = 400 - 0 = 400 → exceeds budget → triggers compaction
        // After level1 (no long tool outputs): still ~647 → triggers level2
        // After level2 (3 old messages summarized): much smaller → fits
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
    // Build a message list that triggers level 3 compaction with tool_call/tool_result
    // groups that could be split across the drop boundary.
    let mut messages = Vec::new();
    // First 2 messages (keep_first=2)
    messages.push(AgentMessage::Llm(Message::user("first")));
    messages.push(AgentMessage::Llm(Message::user("second")));

    // Middle: many assistant+tool pairs that will be dropped
    for i in 0..20 {
        messages.push(make_assistant_with_tool_call(
            &format!("tc-mid-{i}"),
            "bash",
        ));
        messages.push(make_tool_result(&format!("tc-mid-{i}"), "bash"));
    }

    // Recent: a tool pair that could be split at the boundary
    messages.push(make_assistant_with_tool_call("tc-recent", "bash"));
    messages.push(make_tool_result("tc-recent", "bash"));
    messages.push(AgentMessage::Llm(Message::user("last question")));

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

    let (compacted, count) = level1_truncate_tool_outputs(&messages, 20);
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

    let (compacted, count) = level1_truncate_tool_outputs(&messages, 20);
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

    let (compacted, count) = level1_truncate_tool_outputs(&messages, 20);
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

    let (compacted, count) = level1_truncate_tool_outputs(&messages, 20);
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

    let (compacted, count) = level1_truncate_tool_outputs(&messages, 20);
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

    let (compacted, count) = level1_truncate_tool_outputs(&messages, 20);
    assert_eq!(count, 1);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &compacted[2] {
        if let Content::Text { text } = &content[0] {
            assert!(
                text.contains("Structural outline"),
                "expected outline for .py, got: {}",
                &text[..text.len().min(200)]
            );
            assert!(text.len() < py_output.len());
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected tool result");
    }
}
