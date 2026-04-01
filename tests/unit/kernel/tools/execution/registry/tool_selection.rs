//! Tests for tools/selection — verifies tool filter parsing.

use bendclaw::kernel::tools::execution::registry::tool_selection::parse_tool_selection;

#[test]
fn parse_all_returns_none() {
    assert!(parse_tool_selection("all").is_none());
}

#[test]
fn parse_coding_returns_coding_preset() {
    let filter = parse_tool_selection("coding");
    assert!(filter.is_some());
    let names = filter.unwrap();
    assert!(names.contains("bash"), "coding preset should include shell");
    assert!(
        names.contains("read"),
        "coding preset should include file_read"
    );
    assert!(names.contains("grep"), "coding preset should include grep");
}

#[test]
fn parse_comma_separated_returns_exact_names() {
    let filter = parse_tool_selection("read,bash");
    assert!(filter.is_some());
    let names = filter.unwrap();
    assert_eq!(names.len(), 2);
    assert!(names.contains("read"));
    assert!(names.contains("bash"));
}

#[test]
fn parse_empty_inserts_empty_token() {
    // Empty string goes through the "other" branch, inserting ""
    let filter = parse_tool_selection("");
    assert!(filter.is_some());
    let names = filter.unwrap();
    assert_eq!(names.len(), 1);
    assert!(names.contains(""));
}
