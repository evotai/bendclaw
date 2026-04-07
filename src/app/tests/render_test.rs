use bendclaw::cli::repl::render::tool_result_lines;
use bendclaw::cli::repl::render::ToolCallSummary;

#[test]
fn tool_result_lines_preserves_multiline_content() {
    let lines = tool_result_lines("bash", "line 1\nline 2\n\nline 4\n", false, None);
    assert_eq!(lines, vec!["line 1", "line 2", "", "line 4"]);
}

#[test]
fn tool_result_lines_keeps_single_line_summary_behavior() {
    let tool_call = ToolCallSummary {
        name: "read_file".into(),
        summary: "/tmp/demo.txt".into(),
    };
    let lines = tool_result_lines("read_file", "full file contents", false, Some(&tool_call));
    assert_eq!(lines, vec!["Result: /tmp/demo.txt"]);
}

#[test]
fn tool_result_lines_keeps_read_results_compact_even_when_multiline() {
    let tool_call = ToolCallSummary {
        name: "read_file".into(),
        summary: "/tmp/demo.txt".into(),
    };
    let lines = tool_result_lines(
        "read_file",
        "[20 lines]\n   1 | first\n   2 | second",
        false,
        Some(&tool_call),
    );
    assert_eq!(lines, vec!["Result: /tmp/demo.txt"]);
}
