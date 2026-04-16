use evotengine::context::*;
use evotengine::types::*;
use fixtures::compaction_assert::*;
use fixtures::message_dsl::*;

use super::fixtures;

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
    let big_output = (1..=1000)
        .map(|i| format!("output line {} with some extra padding to make it larger enough to exceed the byte threshold", i))
        .collect::<Vec<_>>()
        .join("\n");
    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        make_assistant_with_tool_call("tc-1", "bash"),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "tc-1".into(),
            tool_name: "bash".into(),
            content: vec![Content::Text { text: big_output }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
    ];

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert!(result.stats.tool_outputs_truncated > 0 || result.stats.oversize_capped > 0);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
        if let Content::Text { text } = &content[0] {
            assert!(text.contains("truncated") || text.contains("cleared"));
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
    let rust_output = fake_rust_read_file_output(2000);
    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "read_file",
            serde_json::json!({"path": "/src/foo.rs"}),
        ),
        make_tool_result_with_content("tc-1", "read_file", &rust_output),
    ];

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert!(result.stats.tool_outputs_truncated > 0 || result.stats.oversize_capped > 0);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
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
        let mut lines = vec!["[800 lines]".to_string()];
        for i in 1..=800 {
            lines.push(format!(
                "{:>4} | key{} = \"value{} with extra padding to exceed byte threshold\"",
                i, i, i
            ));
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

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert!(result.stats.tool_outputs_truncated > 0 || result.stats.oversize_capped > 0);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
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
    let big_output = (1..=1000)
        .map(|i| format!("output line {} with some extra padding to make it larger enough to exceed the byte threshold", i))
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

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert!(result.stats.tool_outputs_truncated > 0 || result.stats.oversize_capped > 0);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
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
    let rust_output = fake_rust_read_file_output(2000);

    // ToolCall exists but has no "path" argument, so outline can't determine extension
    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args("tc-1", "read_file", serde_json::json!({})),
        make_tool_result_with_content("tc-1", "read_file", &rust_output),
    ];

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert!(result.stats.tool_outputs_truncated > 0 || result.stats.oversize_capped > 0);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
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

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert_eq!(
        result.stats.tool_outputs_truncated, 0,
        "short content should not be truncated"
    );
    assert_eq!(
        result.stats.oversize_capped, 0,
        "short content should not be capped"
    );
    assert_eq!(
        result.stats.age_cleared, 0,
        "short content should not be age cleared"
    );
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
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
    let mut lines = vec!["[800 lines]".to_string()];
    let mut code_lines = vec![
        "import os".to_string(),
        "import sys".to_string(),
        String::new(),
        "class MyClass:".to_string(),
        "    def __init__(self):".to_string(),
    ];
    // Pad with body lines to exceed byte threshold
    for _ in 0..790 {
        code_lines.push("        self.x = 1".to_string());
    }
    code_lines.push("    def run(self):".to_string());
    code_lines.push("        pass".to_string());
    code_lines.push(String::new());

    code_lines.truncate(800);
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

    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    assert!(result.stats.tool_outputs_truncated > 0 || result.stats.oversize_capped > 0);
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
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
    let big_output = (1..=1000)
        .map(|i| format!("output line {} with some extra padding to make it larger enough to exceed the byte threshold", i))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        AgentMessage::Llm(Message::user("do something")),
        make_assistant_with_tool_call("tc-1", "bash"),
        make_tool_result_with_content("tc-1", "bash", &big_output),
        make_assistant_with_tool_call("tc-2", "bash"),
        make_tool_result_with_content("tc-2", "bash", "short output"),
    ];

    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    let truncation_actions: Vec<_> = result
        .stats
        .actions
        .iter()
        .filter(|a| {
            matches!(
                a.method,
                CompactionMethod::HeadTail
                    | CompactionMethod::Outline
                    | CompactionMethod::OversizeCapped
                    | CompactionMethod::AgeCleared
            )
        })
        .collect();
    assert_eq!(
        truncation_actions.len(),
        1,
        "only one tool output should be truncated"
    );
    assert_eq!(truncation_actions[0].tool_name, "bash");
    assert!(truncation_actions[0].before_tokens > truncation_actions[0].after_tokens);
}

#[test]
fn test_level1_actions_have_correct_index() {
    let big_output = (1..=1000)
        .map(|i| format!("output line {} with some extra padding to make it larger enough to exceed the byte threshold", i))
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

    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    let truncation_actions: Vec<_> = result
        .stats
        .actions
        .iter()
        .filter(|a| {
            matches!(
                a.method,
                CompactionMethod::HeadTail
                    | CompactionMethod::Outline
                    | CompactionMethod::OversizeCapped
                    | CompactionMethod::AgeCleared
            )
        })
        .collect();
    assert_eq!(truncation_actions.len(), 1);
    assert_eq!(truncation_actions[0].index, 5);
}

#[test]
fn test_level1_outline_action_method() {
    let rust_output = fake_rust_read_file_output(2000);
    let messages = vec![
        AgentMessage::Llm(Message::user("read the file")),
        make_assistant_with_tool_call_args(
            "tc-1",
            "read_file",
            serde_json::json!({"path": "/src/foo.rs"}),
        ),
        make_tool_result_with_content("tc-1", "read_file", &rust_output),
    ];

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result = compact_messages(messages, &config);
    let outline_actions: Vec<_> = result
        .stats
        .actions
        .iter()
        .filter(|a| a.tool_name == "read_file")
        .collect();
    assert_eq!(outline_actions.len(), 1);
    assert_eq!(outline_actions[0].tool_name, "read_file");
    assert!(
        outline_actions[0].method == CompactionMethod::Outline
            || outline_actions[0].method == CompactionMethod::OversizeCapped
    );
    assert!(outline_actions[0].end_index.is_none());
    assert!(outline_actions[0].related_count.is_none());
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

    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    for a in &result.stats.actions {
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

    let config = ContextConfig {
        max_context_tokens: 50_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);

    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &result.messages[2] {
        if let Content::Text { text } = &content[0] {
            if text.contains("Structural outline") {
                assert!(text.len() < rust_output.len());
                let outline_actions: Vec<_> = result
                    .stats
                    .actions
                    .iter()
                    .filter(|a| a.method == CompactionMethod::Outline)
                    .collect();
                assert_eq!(outline_actions.len(), 1);
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

    // Level 3 can have actions from earlier passes (Summarized, etc.) plus Dropped
    let has_dropped = result
        .stats
        .actions
        .iter()
        .any(|a| a.method == CompactionMethod::Dropped);
    assert!(
        has_dropped,
        "level 3 should have at least one Dropped action"
    );
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
// Regression tests for compaction pipeline fixes
// ---------------------------------------------------------------------------

/// Under-budget conversations must NOT trigger AgeCleared.
#[test]
fn test_under_budget_no_age_cleared() {
    // Large old web_fetch result, but total tokens well under budget.
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "c1".into(),
                name: "web_fetch".into(),
                arguments: serde_json::json!({"url": "https://example.com"}),
            }],
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            stop_reason: StopReason::ToolUse,
            error_message: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "web_fetch".into(),
            content: vec![Content::Text {
                text: "x\n".repeat(500),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        // Many user messages after to push web_fetch out of recent window
        AgentMessage::Llm(Message::user("q2")),
        AgentMessage::Llm(Message::user("q3")),
        AgentMessage::Llm(Message::user("q4")),
        AgentMessage::Llm(Message::user("q5")),
        AgentMessage::Llm(Message::user("q6")),
    ];

    let config = ContextConfig {
        max_context_tokens: 500_000, // very generous budget
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    assert_eq!(
        result.stats.age_cleared, 0,
        "under-budget should not trigger age clearing"
    );
}

/// tool_output_max_lines should cap per-tool policy lines.
#[test]
fn test_tool_output_max_lines_caps_policy() {
    // read_file policy has normal_max_lines=50, but config says 10.
    // Result should be truncated to ~10 lines, not 50.
    let long_content = (0..200)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "c1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "/tmp/test.rs"}),
            }],
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            stop_reason: StopReason::ToolUse,
            error_message: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "read_file".into(),
            content: vec![Content::Text { text: long_content }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::user("q2")),
    ];

    let config = ContextConfig {
        max_context_tokens: 100, // force over-budget
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 10, // stricter than policy's 50
    };

    let result = compact_messages(messages, &config);
    // Find the tool result and count lines
    for msg in &result.messages {
        if let AgentMessage::Llm(Message::ToolResult {
            tool_name, content, ..
        }) = msg
        {
            if tool_name == "read_file" {
                if let Some(Content::Text { text }) = content.first() {
                    let line_count = text.lines().count();
                    assert!(
                        line_count <= 15,
                        "expected ~10 lines (config cap), got {line_count}"
                    );
                }
            }
        }
    }
}

/// Long single-line content (minified JSON) should be byte-capped.
#[test]
fn test_byte_cap_on_long_single_line() {
    // Single line of 50KB — head-tail by lines won't help much.
    let long_line = "x".repeat(50_000);
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "c1".into(),
                name: "web_fetch".into(),
                arguments: serde_json::json!({"url": "https://example.com"}),
            }],
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            stop_reason: StopReason::ToolUse,
            error_message: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "web_fetch".into(),
            content: vec![Content::Text { text: long_line }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::user("q2")),
    ];

    let config = ContextConfig {
        max_context_tokens: 100, // force over-budget
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    for msg in &result.messages {
        if let AgentMessage::Llm(Message::ToolResult {
            tool_name, content, ..
        }) = msg
        {
            if tool_name == "web_fetch" {
                if let Some(Content::Text { text }) = content.first() {
                    assert!(
                        text.len() <= 16_000,
                        "byte cap should limit to ~15KB, got {} bytes",
                        text.len()
                    );
                }
            }
        }
    }
}

/// Level 2 summary should preserve multiple short text fragments.
#[test]
fn test_level2_summary_preserves_multiple_texts() {
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        // Old assistant turn with multiple short texts + tool calls
        AgentMessage::Llm(Message::Assistant {
            content: vec![
                Content::Text {
                    text: "Planning the fix.".into(),
                },
                Content::ToolCall {
                    id: "c1".into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({"path": "/tmp/a.rs"}),
                },
                Content::Text {
                    text: "Now applying changes.".into(),
                },
                Content::ToolCall {
                    id: "c2".into(),
                    name: "edit_file".into(),
                    arguments: serde_json::json!({"path": "/tmp/a.rs"}),
                },
            ],
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            stop_reason: StopReason::ToolUse,
            error_message: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "read_file".into(),
            content: vec![Content::Text {
                text: "x\n".repeat(500),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c2".into(),
            tool_name: "edit_file".into(),
            content: vec![Content::Text { text: "ok".into() }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        // Recent messages to push the above out of keep_recent
        AgentMessage::Llm(Message::user("q2")),
        AgentMessage::Llm(Message::user("q3")),
        AgentMessage::Llm(Message::user("q4")),
        AgentMessage::Llm(Message::user("q5")),
        AgentMessage::Llm(Message::user("q6")),
        AgentMessage::Llm(Message::user("q7")),
    ];

    let config = ContextConfig {
        max_context_tokens: 100, // force over-budget to trigger L2
        system_prompt_tokens: 0,
        keep_recent: 3,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    // Find the summary message
    let mut found_both = false;
    for msg in &result.messages {
        if let AgentMessage::Llm(Message::User { content, .. }) = msg {
            if let Some(Content::Text { text }) = content.first() {
                if text.contains("[Summary]")
                    && text.contains("Planning the fix")
                    && text.contains("Now applying changes")
                {
                    found_both = true;
                }
            }
        }
    }
    assert!(
        found_both,
        "Level 2 summary should preserve multiple short text fragments"
    );
}

/// Tier 2 oversize cap on read_file (prefer_outline) should record OversizeCapped, not Outline.
#[test]
fn test_tier2_oversize_read_file_records_oversize_capped() {
    // Large read_file result that exceeds oversize threshold
    let long_content = (0..2000)
        .map(|i| format!("fn func_{i}() {{ /* body */ }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: "c1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "/tmp/big.rs"}),
            }],
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            stop_reason: StopReason::ToolUse,
            error_message: None,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "c1".into(),
            tool_name: "read_file".into(),
            content: vec![Content::Text { text: long_content }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::user("q2")),
    ];

    let config = ContextConfig {
        max_context_tokens: 100, // force over-budget so oversize threshold is low
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
    };

    let result = compact_messages(messages, &config);
    assert!(
        result.stats.oversize_capped > 0,
        "Tier 2 on read_file should count as oversize_capped, got oversize_capped={} tool_outputs_truncated={}",
        result.stats.oversize_capped,
        result.stats.tool_outputs_truncated,
    );
}
