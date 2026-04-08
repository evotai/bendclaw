use bendclaw::cli::repl::render::count_messages_by_role;
use bendclaw::cli::repl::render::format_llm_call_lines;
use bendclaw::cli::repl::render::tool_result_lines;
use bendclaw::cli::repl::render::ToolCallSummary;

#[test]
fn tool_result_lines_preserves_multiline_content() {
    let lines = tool_result_lines("line 1\nline 2\n\nline 4\n", false, None);
    assert_eq!(lines, vec!["line 1", "line 2", "", "line 4"]);
}

#[test]
fn tool_result_lines_keeps_single_line_summary_behavior() {
    let tool_call = ToolCallSummary {
        name: "read_file".into(),
        summary: "/tmp/demo.txt".into(),
    };
    let lines = tool_result_lines("full file contents", false, Some(&tool_call));
    assert_eq!(lines, vec!["Result: /tmp/demo.txt"]);
}

#[test]
fn tool_result_lines_keeps_read_results_compact_even_when_multiline() {
    let tool_call = ToolCallSummary {
        name: "read_file".into(),
        summary: "/tmp/demo.txt".into(),
    };
    let lines = tool_result_lines(
        "[20 lines]\n   1 | first\n   2 | second",
        false,
        Some(&tool_call),
    );
    assert_eq!(lines, vec!["Result: /tmp/demo.txt"]);
}

// ---------------------------------------------------------------------------
// count_messages_by_role
// ---------------------------------------------------------------------------

#[test]
fn count_messages_by_role_splits_by_role() {
    let messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": "hi there"}),
        serde_json::json!({"role": "user", "content": "do something"}),
        serde_json::json!({"role": "toolResult", "content": "file contents here"}),
        serde_json::json!({"role": "toolResult", "content": "search results"}),
    ];
    let stats = count_messages_by_role(&messages);
    assert_eq!(stats.user_count, 2);
    assert_eq!(stats.assistant_count, 1);
    assert_eq!(stats.tool_result_count, 2);
    assert_eq!(stats.total_count(), 5);
    assert!(stats.user_tokens > 0);
    assert!(stats.assistant_tokens > 0);
    assert!(stats.tool_result_tokens > 0);
}

#[test]
fn count_messages_by_role_empty() {
    let stats = count_messages_by_role(&[]);
    assert_eq!(stats.total_count(), 0);
    assert_eq!(stats.total_tokens(100), 100);
}

#[test]
fn count_messages_by_role_unknown_role_counts_as_user() {
    let messages: Vec<serde_json::Value> =
        vec![serde_json::json!({"role": "system", "content": "you are helpful"})];
    let stats = count_messages_by_role(&messages);
    assert_eq!(stats.user_count, 1);
    assert_eq!(stats.assistant_count, 0);
    assert_eq!(stats.tool_result_count, 0);
}

#[test]
fn count_messages_by_role_handles_tool_variant_names() {
    let messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "tool_result", "content": "a"}),
        serde_json::json!({"role": "tool", "content": "b"}),
        serde_json::json!({"role": "toolResult", "content": "c"}),
    ];
    let stats = count_messages_by_role(&messages);
    assert_eq!(stats.tool_result_count, 3);
}

// ---------------------------------------------------------------------------
// format_llm_call_lines
// ---------------------------------------------------------------------------

#[test]
fn format_llm_call_lines_basic() {
    let messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "user", "content": "hello world"}),
        serde_json::json!({"role": "assistant", "content": "hi"}),
    ];
    let stats = count_messages_by_role(&messages);
    let lines = format_llm_call_lines(&stats, 3, 495);

    let msg_line = &lines[0];
    let token_line = &lines[1];

    assert!(msg_line.contains("2 messages"));
    assert!(msg_line.contains("user 1"));
    assert!(msg_line.contains("assistant 1"));
    assert!(!msg_line.contains("tool_result"));
    assert!(msg_line.contains("3 tools"));

    assert!(token_line.contains("est tokens"));
    assert!(token_line.contains("sys ~495"));
    assert!(token_line.contains("user ~"));
    assert!(token_line.contains("assistant ~"));
    assert!(!token_line.contains("tool_result"));
}

#[test]
fn format_llm_call_lines_with_tool_results() {
    let messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "user", "content": "read the file"}),
        serde_json::json!({"role": "assistant", "content": "sure"}),
        serde_json::json!({"role": "toolResult", "toolName": "read", "content": "file data here"}),
    ];
    let stats = count_messages_by_role(&messages);
    let lines = format_llm_call_lines(&stats, 6, 500);

    let msg_line = &lines[0];
    let token_line = &lines[1];

    assert!(msg_line.contains("3 messages"));
    assert!(msg_line.contains("tool_result 1"));
    assert!(msg_line.contains("6 tools"));

    assert!(token_line.contains("tool_result ~"));
}

#[test]
fn format_llm_call_lines_empty_messages() {
    let stats = count_messages_by_role(&[]);
    let lines = format_llm_call_lines(&stats, 0, 200);

    let msg_line = &lines[0];
    let token_line = &lines[1];

    assert!(msg_line.contains("0 messages"));
    assert!(msg_line.contains("0 tools"));
    assert!(token_line.contains("~200 est tokens"));
    assert!(token_line.contains("sys ~200"));
}

#[test]
fn tool_result_lines_truncates_large_output() {
    let big_content: String = (0..100)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let lines = tool_result_lines(&big_content, false, None);
    assert_eq!(lines.len(), 31); // 30 lines + 1 truncation notice
    assert!(lines[30].contains("70 more lines truncated"));
}

#[test]
fn tool_result_lines_no_truncation_under_limit() {
    let content: String = (0..20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let lines = tool_result_lines(&content, false, None);
    assert_eq!(lines.len(), 20);
}
