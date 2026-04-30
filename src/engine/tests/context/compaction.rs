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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    if result.stats.level == 2 {
        assert_actions_match_level(2, &result.stats.actions);
    }
}

#[test]
fn test_compact_level0_no_actions() {
    let messages = pat("u a u").pad(10).build();
    let config = ContextConfig::default();
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert!(!result.messages.is_empty());
}

#[test]
fn test_compact_all_users_no_tool() {
    let messages = pat("u a u a u").pad(500).build();
    let config = ContextConfig {
        max_context_tokens: 200,
        system_prompt_tokens: 0,
        keep_recent: 1,
        keep_first: 0,
        tool_output_max_lines: 50,
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };
    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        tool_output_max_lines: 10,
        ..Default::default() // stricter than policy's 50
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
    let long_line = "x".repeat(5_000);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    // Find the summary message
    let mut found_both = false;
    for msg in &result.messages {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
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
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);
    assert!(
        result.stats.oversize_capped > 0,
        "Tier 2 on read_file should count as oversize_capped, got oversize_capped={} tool_outputs_truncated={}",
        result.stats.oversize_capped,
        result.stats.tool_outputs_truncated,
    );
}

// ---------------------------------------------------------------------------
// Regression: provider-reported tokens exceed chars/4 estimate
// ---------------------------------------------------------------------------

/// When the provider reports higher token usage than chars/4 estimates,
/// compact must still trigger L1+ passes. This reproduces the bug where
/// compact saw ~85k (chars/4) while the provider reported ~167k, causing
/// a no-op even though the context exceeded the budget.
#[test]
fn test_compact_triggers_when_provider_estimate_exceeds_char_estimate() {
    // Build messages whose chars/4 estimate fits within budget,
    // but simulate a provider reporting much higher token usage.
    let messages =
        pat("u a t r u a t r u a t r u a t r u a t r u a t r u a t r u a t r u a t r u a t r")
            .pad(50)
            .tool_output(200)
            .build();

    let char_estimate = total_tokens(&messages);

    let config = ContextConfig {
        // Set budget so chars/4 fits but provider estimate doesn't
        max_context_tokens: char_estimate + 1000,
        system_prompt_tokens: 0,
        keep_recent: 4,
        keep_first: 2,
        tool_output_max_lines: 20,
        ..Default::default()
    };

    // With chars/4 estimate: should be a no-op (within budget)
    let budget_within = CompactionBudgetState::from_messages(&messages);
    let result_noop = compact_messages(messages.clone(), &config, &budget_within);
    assert_eq!(
        result_noop.stats.level, 0,
        "chars/4 estimate should be within budget"
    );

    // Simulate provider reporting 2x the chars/4 estimate (e.g. non-English, structured tokens)
    let provider_estimate = char_estimate * 2;
    let budget_over = CompactionBudgetState {
        estimated_tokens: provider_estimate,
    };
    let result_compact = compact_messages(messages, &config, &budget_over);
    assert!(
        result_compact.stats.level >= 1,
        "provider estimate exceeds budget, compact should trigger L1+, got level={}",
        result_compact.stats.level,
    );
    assert!(
        result_compact.stats.after_estimated_tokens < provider_estimate,
        "compaction should reduce estimated tokens: before={} after={}",
        provider_estimate,
        result_compact.stats.after_estimated_tokens,
    );
}

// ---------------------------------------------------------------------------
// Oversized user message truncation (L0)
// ---------------------------------------------------------------------------

/// Old oversized user message should be truncated when over budget.
#[test]
fn test_oversized_old_user_message_truncated() {
    let big_text = (1..=2000)
        .map(|i| format!("line {} with padding to make it large enough", i))
        .collect::<Vec<_>>()
        .join("\n");

    // Budget smaller than the big user message but large enough that
    // L1/L2 don't interfere (only 5 messages, keep_recent=3 keeps the tail).
    let char_tokens = big_text.len() / 4;

    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::User {
            content: vec![Content::Text {
                text: big_text.clone(),
            }],
            timestamp: 0,
        }),
        AgentMessage::Llm(Message::user("q3")),
        AgentMessage::Llm(Message::user("q4")),
        AgentMessage::Llm(Message::user("q5")),
    ];

    let config = ContextConfig {
        max_context_tokens: char_tokens / 2,
        system_prompt_tokens: 0,
        keep_recent: 3,
        keep_first: 1,
        tool_output_max_lines: 20,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

    if let AgentMessage::Llm(Message::User { content, .. }) = &result.messages[1] {
        if let Content::Text { text } = &content[0] {
            assert!(
                text.len() < big_text.len(),
                "oversized old user message should be truncated"
            );
            assert!(
                text.contains("truncated"),
                "truncated text should contain truncation marker"
            );
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected user message at index 1");
    }
}

/// Recent user message should NOT be truncated even if oversized.
#[test]
fn test_recent_oversized_user_message_kept() {
    let big_text = (1..=2000)
        .map(|i| format!("line {} with padding to make it large enough", i))
        .collect::<Vec<_>>()
        .join("\n");

    // All messages are recent (keep_recent=10 > 2 messages), so L0 user
    // truncation won't fire. Budget is large enough that L1/L2 don't trigger.
    let char_tokens = big_text.len() / 4;

    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::User {
            content: vec![Content::Text {
                text: big_text.clone(),
            }],
            timestamp: 0,
        }),
    ];

    let config = ContextConfig {
        max_context_tokens: char_tokens * 2,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 1,
        tool_output_max_lines: 20,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

    if let AgentMessage::Llm(Message::User { content, .. }) = &result.messages[1] {
        if let Content::Text { text } = &content[0] {
            assert_eq!(
                text.len(),
                big_text.len(),
                "recent user message should not be truncated"
            );
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected user message at index 1");
    }
}

/// Pinned (keep_first) user message should NOT be truncated by L0 even if oversized.
#[test]
fn test_pinned_oversized_user_message_kept() {
    let big_text = (1..=2000)
        .map(|i| format!("line {} with padding to make it large enough", i))
        .collect::<Vec<_>>()
        .join("\n");

    // All messages are recent and within chars/4 budget, so L1/L2 are no-ops.
    // We inflate the provider estimate so L0 sees over-budget, but the pinned
    // message (index 0) must survive L0 due to keep_first.
    let messages = vec![
        AgentMessage::Llm(Message::User {
            content: vec![Content::Text {
                text: big_text.clone(),
            }],
            timestamp: 0,
        }),
        AgentMessage::Llm(Message::user("q2")),
        AgentMessage::Llm(Message::user("q3")),
        AgentMessage::Llm(Message::user("q4")),
        AgentMessage::Llm(Message::user("q5")),
    ];

    let char_tokens = total_tokens(&messages);

    let config = ContextConfig {
        // Budget above chars/4 total so L1/L2 gates don't fire.
        max_context_tokens: char_tokens + 10_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 1,
        tool_output_max_lines: 20,
        ..Default::default()
    };

    // Provider estimate far exceeds budget → L0 sees over-budget.
    let budget_state = CompactionBudgetState {
        estimated_tokens: char_tokens * 4,
    };
    let result = compact_messages(messages, &config, &budget_state);

    // The pinned message at index 0 must be unchanged by L0.
    if let AgentMessage::Llm(Message::User { content, .. }) = &result.messages[0] {
        if let Content::Text { text } = &content[0] {
            assert_eq!(
                text.len(),
                big_text.len(),
                "pinned user message should not be truncated"
            );
        } else {
            panic!("expected text content");
        }
    } else {
        panic!("expected user message at index 0");
    }
}

/// Oversized user message with multiple text blocks should be merged and truncated as a whole.
#[test]
fn test_oversized_user_multi_block_merged() {
    let block = (1..=1000)
        .map(|i| format!("block line {}", i))
        .collect::<Vec<_>>()
        .join("\n");

    let char_tokens = block.len() / 2; // two blocks total

    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::User {
            content: vec![
                Content::Text {
                    text: block.clone(),
                },
                Content::Text {
                    text: block.clone(),
                },
            ],
            timestamp: 0,
        }),
        AgentMessage::Llm(Message::user("q3")),
        AgentMessage::Llm(Message::user("q4")),
        AgentMessage::Llm(Message::user("q5")),
    ];

    let config = ContextConfig {
        max_context_tokens: char_tokens / 2,
        system_prompt_tokens: 0,
        keep_recent: 3,
        keep_first: 1,
        tool_output_max_lines: 20,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

    if let AgentMessage::Llm(Message::User { content, .. }) = &result.messages[1] {
        // Should be merged into a single text block
        assert_eq!(
            content.len(),
            1,
            "multiple text blocks should be merged into one, got {}",
            content.len()
        );
        if let Content::Text { text } = &content[0] {
            assert!(
                text.contains("truncated"),
                "merged text should be truncated"
            );
        }
    } else {
        panic!("expected user message at index 1");
    }
}

/// Oversized user message with images should preserve images and truncate text.
#[test]
fn test_oversized_user_with_images_preserved() {
    let big_text = (1..=2000)
        .map(|i| format!("line {} with padding to make it large enough", i))
        .collect::<Vec<_>>()
        .join("\n");

    let char_tokens = big_text.len() / 4;

    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::User {
            content: vec![Content::Text { text: big_text }, Content::Image {
                data: "base64data".into(),
                mime_type: "image/png".into(),
            }],
            timestamp: 0,
        }),
        AgentMessage::Llm(Message::user("q3")),
        AgentMessage::Llm(Message::user("q4")),
        AgentMessage::Llm(Message::user("q5")),
    ];

    let config = ContextConfig {
        max_context_tokens: char_tokens / 2,
        system_prompt_tokens: 0,
        keep_recent: 3,
        keep_first: 1,
        tool_output_max_lines: 20,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

    if let AgentMessage::Llm(Message::User { content, .. }) = &result.messages[1] {
        // Text block (truncated) + image block preserved
        assert!(content.len() >= 2, "expected text + image");
        if let Content::Text { text } = &content[0] {
            assert!(text.contains("truncated"), "text should be truncated");
        }
        let has_image = content.iter().any(|c| matches!(c, Content::Image { .. }));
        assert!(has_image, "image should be preserved, not stripped");
    } else {
        panic!("expected user message at index 1");
    }
}

/// User message under budget should not be truncated regardless of size.
#[test]
fn test_user_message_under_budget_not_truncated() {
    let big_text = (1..=500)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::User {
            content: vec![Content::Text {
                text: big_text.clone(),
            }],
            timestamp: 0,
        }),
        AgentMessage::Llm(Message::user("q3")),
        AgentMessage::Llm(Message::user("q4")),
        AgentMessage::Llm(Message::user("q5")),
    ];

    let config = ContextConfig {
        max_context_tokens: 999_999,
        system_prompt_tokens: 0,
        keep_recent: 3,
        keep_first: 1,
        tool_output_max_lines: 20,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

    if let AgentMessage::Llm(Message::User { content, .. }) = &result.messages[1] {
        if let Content::Text { text } = &content[0] {
            assert_eq!(
                text.len(),
                big_text.len(),
                "under-budget user message should not be truncated"
            );
        }
    } else {
        panic!("expected user message at index 1");
    }
}

// ---------------------------------------------------------------------------
// Compact trigger/target: long-session "toothpaste squeezing" prevention
// ---------------------------------------------------------------------------

/// Simulate a long session where context hovers near budget.
///
/// Before the compact_target fix, L1 would stop as soon as context dipped
/// below the trigger threshold (80%), leading to per-turn "toothpaste squeezing".
/// With compact_target at 75%, L1 summarizes enough turns to push context
/// well below the trigger, keeping the context healthy.
#[test]
fn test_l1_compacts_to_target() {
    // Build a conversation with many tool turns.
    let budget = 8_000;
    let trigger_expected = budget * 80 / 100;
    let target_expected = budget * 75 / 100;

    let mut messages = Vec::new();
    let mut tool_id = 0usize;

    // Initial user message
    messages.push(AgentMessage::Llm(Message::user("implement the feature")));

    // Build turns to fill context above trigger
    for i in 0..80 {
        tool_id += 1;
        let id = format!("tc-{}", tool_id);
        messages.push(AgentMessage::Llm(Message::Assistant {
            content: vec![
                Content::Text {
                    text: format!("Step {} — let me check this file", i),
                },
                Content::ToolCall {
                    id: id.clone(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({"path": format!("/src/file_{}.rs", i)}),
                },
            ],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }));
        messages.push(AgentMessage::Llm(Message::ToolResult {
            tool_call_id: id,
            tool_name: "read_file".into(),
            content: vec![Content::Text {
                text: format!("fn func_{}() {{\n{}\n}}", i, "    // code\n".repeat(30)),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }));

        if i % 20 == 19 {
            messages.push(AgentMessage::Llm(Message::user("continue")));
        }
    }

    let config = ContextConfig {
        max_context_tokens: budget,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let est_tokens = total_tokens(&messages);

    // Only run the test if we're actually near/above the trigger
    // (otherwise the test setup doesn't match the scenario)
    assert!(
        est_tokens > trigger_expected,
        "test setup: need est_tokens ({}) > trigger ({})",
        est_tokens,
        trigger_expected
    );

    let budget_state = CompactionBudgetState {
        estimated_tokens: est_tokens,
    };
    let result = compact_messages(messages.clone(), &config, &budget_state);

    // L1 should have triggered: turns_summarized > 0
    assert!(
        result.stats.turns_summarized > 0,
        "L1 should trigger when context is above trigger ({}): \
         est_tokens={}, level={}, turns_summarized={}",
        trigger_expected,
        est_tokens,
        result.stats.level,
        result.stats.turns_summarized,
    );

    // After compaction, tokens should be at or below compact_target (75%)
    assert!(
        result.stats.after_estimated_tokens <= target_expected,
        "after L1, tokens ({}) should be <= target ({})",
        result.stats.after_estimated_tokens,
        target_expected,
    );

    // Message count should have decreased
    assert!(
        result.stats.after_message_count < result.stats.before_message_count,
        "L1 should reduce message count: before={}, after={}",
        result.stats.before_message_count,
        result.stats.after_message_count,
    );

    assert_no_orphan_tool_pairs(&result.messages);
    assert_actions_match_level(result.stats.level, &result.stats.actions);
}

/// Simulate multiple compaction rounds (like a real long session).
///
/// Each round: compact, then add a new turn, repeat.
/// Verify that L1 prevents the "toothpaste squeezing" pattern where
/// message count grows monotonically.
#[test]
fn test_multi_round_compaction_prevents_toothpaste_squeezing() {
    let budget = 3_000;
    let config = ContextConfig {
        max_context_tokens: budget,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let mut messages = Vec::new();
    let mut tool_id = 0usize;

    // Seed with initial user message
    messages.push(AgentMessage::Llm(Message::user("start the task")));

    let mut l1_triggered_count = 0;
    let mut max_message_count = 0usize;

    // Simulate 60 turns
    for i in 0..60 {
        // Add a new turn: assistant + tool_call + tool_result
        tool_id += 1;
        let id = format!("tc-{}", tool_id);
        messages.push(AgentMessage::Llm(Message::Assistant {
            content: vec![
                Content::Text {
                    text: format!("Working on step {}", i),
                },
                Content::ToolCall {
                    id: id.clone(),
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "cargo build"}),
                },
            ],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }));
        messages.push(AgentMessage::Llm(Message::ToolResult {
            tool_call_id: id,
            tool_name: "bash".into(),
            content: vec![Content::Text {
                text: format!("output line {}\n{}", i, "data ".repeat(50)),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }));

        // Run compaction
        let est = total_tokens(&messages);
        let budget_state = CompactionBudgetState {
            estimated_tokens: est,
        };
        let result = compact_messages(messages, &config, &budget_state);

        if result.stats.turns_summarized > 0 {
            l1_triggered_count += 1;
        }

        messages = result.messages;
        if messages.len() > max_message_count {
            max_message_count = messages.len();
        }
    }

    // L1 should have triggered at least once during 60 turns
    assert!(
        l1_triggered_count > 0,
        "L1 should trigger at least once during a 60-turn session"
    );

    // Message count should be bounded — not growing to 120+ (2 msgs per turn × 60)
    // With L1 active, it should stay well below the theoretical max
    let theoretical_max = 1 + 60 * 2; // initial user + 60 turns × (assistant + tool_result)
    assert!(
        max_message_count < theoretical_max * 3 / 4,
        "message count should be bounded by L1 compaction: max_seen={}, theoretical_max={}",
        max_message_count,
        theoretical_max,
    );

    // Final message count should be reasonable
    assert!(
        messages.len() < 100,
        "final message count should be reasonable: got {}",
        messages.len(),
    );
}

/// When context is below trigger, L1 should NOT trigger.
/// This ensures we don't over-compact healthy sessions.
#[test]
fn test_l1_does_not_trigger_below_target() {
    // Small conversation well under budget
    let messages = pat("u tr u tr u tr u tr u tr")
        .pad(100)
        .tool_output(200)
        .build();

    let config = ContextConfig {
        max_context_tokens: 100_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);

    assert_eq!(
        result.stats.turns_summarized, 0,
        "L1 should not trigger when well under budget"
    );
    assert_eq!(
        result.stats.after_message_count, result.stats.before_message_count,
        "no messages should be removed when under budget"
    );
}

// ---------------------------------------------------------------------------
// L1 → L2 escalation and extreme cases
// ---------------------------------------------------------------------------

/// When L1 collapse is not enough (all old turns already tiny), L2 evict kicks in.
#[test]
fn test_l2_triggers_when_l1_insufficient() {
    // Many small assistant messages (no tool calls) — L1 summarize saves almost nothing
    // because summaries are similar size to originals.
    let mut messages = Vec::new();
    messages.push(AgentMessage::Llm(Message::user("x".repeat(500))));
    messages.push(AgentMessage::Llm(Message::user("x".repeat(500))));

    for i in 0..40 {
        messages.push(AgentMessage::Llm(Message::Assistant {
            content: vec![Content::Text {
                text: format!("ok {}", i), // tiny — summary won't save much
            }],
            stop_reason: StopReason::Stop,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }));
    }
    messages.push(AgentMessage::Llm(Message::user("final")));

    let config = ContextConfig {
        max_context_tokens: 400,
        system_prompt_tokens: 0,
        keep_recent: 3,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);

    // Should escalate to L2 (evict/drop)
    assert!(
        result.stats.messages_dropped > 0 || result.stats.after_message_count < messages.len(),
        "L2 should trigger when L1 cannot reduce enough: level={}, dropped={}, before={}, after={}",
        result.stats.level,
        result.stats.messages_dropped,
        messages.len(),
        result.stats.after_message_count,
    );

    assert!(
        result.stats.after_estimated_tokens <= 400,
        "after L2, tokens ({}) should be within budget (400)",
        result.stats.after_estimated_tokens,
    );

    assert_no_orphan_tool_pairs(&result.messages);
    assert_actions_match_level(result.stats.level, &result.stats.actions);
}

/// L1 and L2 cooperate: L1 summarizes what it can, L2 drops the rest.
#[test]
fn test_l1_and_l2_cooperate() {
    // Mix of large tool turns (L1 can summarize) and many small messages (L2 drops)
    let mut messages = Vec::new();
    let mut tool_id = 0usize;

    messages.push(AgentMessage::Llm(Message::user("start")));

    // 10 large tool turns — L1 can summarize these
    for i in 0..10 {
        tool_id += 1;
        let id = format!("tc-{}", tool_id);
        messages.push(AgentMessage::Llm(Message::Assistant {
            content: vec![
                Content::Text {
                    text: format!("checking file {}", i),
                },
                Content::ToolCall {
                    id: id.clone(),
                    name: "bash".into(),
                    arguments: serde_json::json!({}),
                },
            ],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }));
        messages.push(AgentMessage::Llm(Message::ToolResult {
            tool_call_id: id,
            tool_name: "bash".into(),
            content: vec![Content::Text {
                text: "output ".repeat(200),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }));
    }

    // 20 small assistant messages — L1 summary won't save much
    for i in 0..20 {
        messages.push(AgentMessage::Llm(Message::Assistant {
            content: vec![Content::Text {
                text: format!("step {}", i),
            }],
            stop_reason: StopReason::Stop,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }));
    }

    messages.push(AgentMessage::Llm(Message::user("done")));

    let total_est = total_tokens(&messages);
    // Budget is 30% of total — forces both L1 and L2
    let budget = total_est * 30 / 100;

    let config = ContextConfig {
        max_context_tokens: budget,
        system_prompt_tokens: 0,
        keep_recent: 4,
        keep_first: 1,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages.clone(), &config, &budget_state);

    // Both L1 and L2 should have contributed
    let has_summarized = result
        .stats
        .actions
        .iter()
        .any(|a| a.method == CompactionMethod::Summarized);
    let has_dropped = result
        .stats
        .actions
        .iter()
        .any(|a| a.method == CompactionMethod::Dropped);

    assert!(
        has_summarized || has_dropped,
        "at least one of L1 (summarize) or L2 (drop) should act: level={}",
        result.stats.level,
    );

    assert!(
        result.stats.after_message_count < messages.len(),
        "message count should decrease: before={}, after={}",
        messages.len(),
        result.stats.after_message_count,
    );

    assert_no_orphan_tool_pairs(&result.messages);
    assert_actions_match_level(result.stats.level, &result.stats.actions);
}

/// Extreme: single user message larger than budget.
/// Compaction should not panic and should return at least one message.
#[test]
fn test_extreme_single_message_exceeds_budget() {
    let huge_text = "x".repeat(10_000);
    let messages = vec![AgentMessage::Llm(Message::user(&huge_text))];

    let config = ContextConfig {
        max_context_tokens: 1_000,
        system_prompt_tokens: 0,
        keep_recent: 10,
        keep_first: 2,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

    // Should not panic, should return at least one message
    assert!(
        !result.messages.is_empty(),
        "should always return at least one message"
    );
}

/// Extreme: all messages are tool results with no matching calls.
/// After sanitize, orphans get removed. Compaction should handle gracefully.
#[test]
fn test_extreme_all_orphan_tool_results() {
    let messages = vec![
        AgentMessage::Llm(Message::user("task")),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "orphan-1".into(),
            tool_name: "bash".into(),
            content: vec![Content::Text {
                text: "x".repeat(5000),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: "orphan-2".into(),
            tool_name: "bash".into(),
            content: vec![Content::Text {
                text: "y".repeat(5000),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }),
        AgentMessage::Llm(Message::user("end")),
    ];

    let config = ContextConfig {
        max_context_tokens: 500,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 1,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

    // Orphan tool results should be sanitized away
    let has_orphan_results = result.messages.iter().any(|m| {
        matches!(
            m,
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id,
                ..
            }) if tool_call_id.starts_with("orphan")
        )
    });
    assert!(
        !has_orphan_results,
        "orphan tool results should be removed by sanitize"
    );
    assert_no_orphan_tool_pairs(&result.messages);
}

/// Multi-round simulation where user-only history grows past the hard message limit.
/// In that case L1 may conservatively summarize old consecutive user messages
/// before L2 eviction has to drop them entirely.
#[test]
fn test_multi_round_l2_escalation() {
    let budget = 800;
    let config = ContextConfig {
        max_context_tokens: budget,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 1,
        tool_output_max_lines: 50,
        max_messages_hard: 8,
        ..Default::default()
    };

    let mut messages = Vec::new();
    messages.push(AgentMessage::Llm(Message::user("start")));

    let mut l1_summarized = false;

    for i in 0..20 {
        messages.push(AgentMessage::Llm(Message::user(format!(
            "question {} {}",
            i,
            "context ".repeat(80)
        ))));

        let est = total_tokens(&messages);
        let budget_state = CompactionBudgetState {
            estimated_tokens: est,
        };
        let result = compact_messages(messages, &config, &budget_state);

        if result.stats.turns_summarized > 0 {
            l1_summarized = true;
        }

        messages = result.messages;
    }

    // L1 should collapse old consecutive user messages only after hard message pressure,
    // preserving snippets instead of letting L2 drop them wholesale.
    assert!(
        l1_summarized,
        "L1 should collapse old consecutive user messages before L2 eviction"
    );

    let final_tokens = total_tokens(&messages);
    assert!(
        final_tokens <= budget,
        "final tokens ({}) should be within budget ({})",
        final_tokens,
        budget,
    );
}

// ---------------------------------------------------------------------------
// L1 (level 2) → L2 (level 3) escalation tests
// ---------------------------------------------------------------------------

/// L1 summarizes old turns but still over budget → L2 drops middle messages.
/// Verifies the full escalation path: L0 → L1 → L2 in a single compact call.
#[test]
fn test_l1_to_l2_escalation_single_call() {
    // Many assistant+tool turns with large tool output.
    // Budget is so tight that even after L1 summarizes everything, it's still over.
    let mut messages = Vec::new();
    let mut tool_id = 0usize;

    // Large initial user message (pinned by keep_first)
    messages.push(AgentMessage::Llm(Message::user("task ".repeat(200))));

    // 30 tool turns — L1 can summarize these but summaries still take space
    for i in 0..30 {
        tool_id += 1;
        let id = format!("tc-{}", tool_id);
        messages.push(AgentMessage::Llm(Message::Assistant {
            content: vec![
                Content::Text {
                    text: format!("checking {}", i),
                },
                Content::ToolCall {
                    id: id.clone(),
                    name: "bash".into(),
                    arguments: serde_json::json!({}),
                },
            ],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }));
        messages.push(AgentMessage::Llm(Message::ToolResult {
            tool_call_id: id,
            tool_name: "bash".into(),
            content: vec![Content::Text {
                text: "result ".repeat(60),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }));
    }

    // Recent user message
    messages.push(AgentMessage::Llm(Message::user("what now?")));

    let est = total_tokens(&messages);
    // Budget = 15% of total — L1 alone can't fit this
    let budget = est * 15 / 100;

    let config = ContextConfig {
        max_context_tokens: budget,
        system_prompt_tokens: 0,
        keep_recent: 4,
        keep_first: 1,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState {
        estimated_tokens: est,
    };
    let result = compact_messages(messages.clone(), &config, &budget_state);

    // Should reach level 3 (L2 evict)
    assert_eq!(
        result.stats.level, 3,
        "should escalate to level 3 (L2 evict): turns_summarized={}, messages_dropped={}",
        result.stats.turns_summarized, result.stats.messages_dropped,
    );

    // Both L1 and L2 should have acted
    assert!(
        result.stats.turns_summarized > 0,
        "L1 should have summarized some turns before L2 kicked in"
    );
    assert!(
        result.stats.messages_dropped > 0,
        "L2 should have dropped messages after L1 was insufficient"
    );

    // Result should fit within budget
    assert!(
        result.stats.after_estimated_tokens <= budget,
        "after full escalation, tokens ({}) should be within budget ({})",
        result.stats.after_estimated_tokens,
        budget,
    );

    assert_no_orphan_tool_pairs(&result.messages);
    assert_actions_match_level(result.stats.level, &result.stats.actions);
}

/// Multi-round simulation where sessions grow so fast that L1 is never enough
/// and L2 must repeatedly fire.
#[test]
fn test_multi_round_repeated_l2_escalation() {
    // Very tight budget with keep_recent covering most messages.
    // L1 boundary (len - keep_recent) is tiny, so L1 has almost nothing to
    // collapse. L2 must drop messages to stay within compact_target (75%).
    let budget = 3_000;
    let _target = budget * 75 / 100;
    let config = ContextConfig {
        max_context_tokens: budget,
        system_prompt_tokens: 0,
        keep_recent: 20, // large keep_recent → L1 boundary is small
        keep_first: 1,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let mut messages = Vec::new();
    messages.push(AgentMessage::Llm(Message::user("start")));

    let mut l2_count = 0;

    for i in 0..40 {
        // Add user messages — L1 doesn't collapse user messages
        // Need enough text per message to fill budget with tiktoken counting
        messages.push(AgentMessage::Llm(Message::user(format!(
            "question {} about the architecture of the system {}",
            i,
            "the quick brown fox jumps over the lazy dog. ".repeat(20)
        ))));

        let est = total_tokens(&messages);
        let budget_state = CompactionBudgetState {
            estimated_tokens: est,
        };
        let result = compact_messages(messages, &config, &budget_state);

        if result.stats.messages_dropped > 0 {
            l2_count += 1;
        }

        messages = result.messages;
    }

    assert!(
        l2_count > 0,
        "L2 should trigger when L1 has nothing to collapse (user-only messages with large keep_recent)"
    );

    let final_tokens = total_tokens(&messages);
    // After repeated L2 eviction, context should be well below original budget.
    // May exceed compact_target if keep_recent messages are individually large.
    assert!(
        final_tokens <= budget,
        "final tokens ({}) should be <= budget ({}) after L2 eviction",
        final_tokens,
        budget,
    );
    // Verify compaction actually reduced context
    assert!(
        messages.len() < 30,
        "should have dropped messages: got {} messages",
        messages.len(),
    );
}

/// Edge case: keep_recent is larger than total messages.
/// L1 boundary = 0, so L1 has nothing to collapse. L2 must handle it.
#[test]
fn test_l2_when_keep_recent_covers_all() {
    let mut messages = Vec::new();
    let mut tool_id = 0usize;

    messages.push(AgentMessage::Llm(Message::user("task")));

    for i in 0..5 {
        tool_id += 1;
        let id = format!("tc-{}", tool_id);
        messages.push(AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: id.clone(),
                name: "bash".into(),
                arguments: serde_json::json!({}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }));
        messages.push(AgentMessage::Llm(Message::ToolResult {
            tool_call_id: id,
            tool_name: "bash".into(),
            content: vec![Content::Text {
                text: format!("big output {} {}", i, "x".repeat(2000)),
            }],
            is_error: false,
            timestamp: 0,
            retention: Retention::Normal,
        }));
    }

    let est = total_tokens(&messages);

    let config = ContextConfig {
        max_context_tokens: est / 3, // very tight
        system_prompt_tokens: 0,
        keep_recent: 100, // larger than message count — L1 boundary = 0
        keep_first: 1,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState {
        estimated_tokens: est,
    };
    let result = compact_messages(messages.clone(), &config, &budget_state);

    // L1 should have nothing to collapse (all messages are "recent")
    assert_eq!(
        result.stats.turns_summarized, 0,
        "L1 should not summarize when keep_recent covers all messages"
    );

    // L2 or L0 should have handled the reduction
    assert!(
        result.stats.after_estimated_tokens < est,
        "compaction should reduce tokens even when keep_recent covers all: before={}, after={}",
        est,
        result.stats.after_estimated_tokens,
    );

    assert_no_orphan_tool_pairs(&result.messages);
}

/// Edge case: budget is zero. Should not panic, should return something.
#[test]
fn test_extreme_zero_budget() {
    let messages = pat("u tr u tr u").pad(100).tool_output(500).build();

    let config = ContextConfig {
        max_context_tokens: 0,
        system_prompt_tokens: 0,
        keep_recent: 2,
        keep_first: 1,
        tool_output_max_lines: 50,
        ..Default::default()
    };

    let budget_state = CompactionBudgetState::from_messages(&messages);
    let result = compact_messages(messages, &config, &budget_state);

    // Should not panic, should return at least one message
    assert!(
        !result.messages.is_empty(),
        "zero budget should still return at least one message"
    );
    assert_no_orphan_tool_pairs(&result.messages);
}
