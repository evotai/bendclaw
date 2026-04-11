use bendclaw::cli::repl::diff::diff_from_details;
use bendclaw::cli::repl::diff::format_diff;

#[test]
fn no_changes_shows_message() {
    let result = format_diff("hello\n", "hello\n");
    assert_eq!(result.lines_added, 0);
    assert_eq!(result.lines_removed, 0);
    assert!(result.text.contains("no changes"));
}

#[test]
fn single_line_insert() {
    let result = format_diff("a\nb\n", "a\nx\nb\n");
    assert_eq!(result.lines_added, 1);
    assert_eq!(result.lines_removed, 0);
    assert!(result.text.contains("+x"));
}

#[test]
fn single_line_delete() {
    let result = format_diff("a\nb\nc\n", "a\nc\n");
    assert_eq!(result.lines_added, 0);
    assert_eq!(result.lines_removed, 1);
    assert!(result.text.contains("-b"));
}

#[test]
fn replace_line() {
    let result = format_diff("a\nold\nc\n", "a\nnew\nc\n");
    assert_eq!(result.lines_added, 1);
    assert_eq!(result.lines_removed, 1);
    assert!(result.text.contains("-old"));
    assert!(result.text.contains("+new"));
}

#[test]
fn diff_from_details_precomputed_diff_field() {
    let details = serde_json::json!({
        "path": "/tmp/foo.rs",
        "diff": "--- a/foo.rs\n+++ b/foo.rs\n@@ -1,3 +1,3 @@\n a\n-old\n+new\n c\n",
    });
    let diff = diff_from_details(&details).unwrap();
    assert!(diff.contains("-old"));
    assert!(diff.contains("+new"));
}

#[test]
fn diff_from_details_empty_diff_returns_none() {
    let details = serde_json::json!({
        "diff": "",
        "path": "/tmp/foo",
    });
    assert!(diff_from_details(&details).is_none());
}

#[test]
fn diff_from_details_no_diff_field_returns_none() {
    let details = serde_json::json!({ "path": "/tmp/foo" });
    assert!(diff_from_details(&details).is_none());
}
