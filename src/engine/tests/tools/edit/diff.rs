use bendengine::tools::edit::diff::*;

#[test]
fn single_line_replace() {
    let old = "aaa\nbbb\nccc\n";
    let new = "aaa\nxxx\nccc\n";
    let d = unified_diff(old, new, "test.rs");
    assert_eq!(d.added_lines, 1);
    assert_eq!(d.removed_lines, 1);
    assert!(d.unified.contains("-bbb"));
    assert!(d.unified.contains("+xxx"));
}

#[test]
fn multi_line_add() {
    let old = "aaa\nccc\n";
    let new = "aaa\nbbb\nccc\n";
    let d = unified_diff(old, new, "test.rs");
    assert_eq!(d.added_lines, 1);
    assert_eq!(d.removed_lines, 0);
    assert!(d.unified.contains("+bbb"));
}

#[test]
fn multi_line_delete() {
    let old = "aaa\nbbb\nccc\n";
    let new = "aaa\nccc\n";
    let d = unified_diff(old, new, "test.rs");
    assert_eq!(d.added_lines, 0);
    assert_eq!(d.removed_lines, 1);
    assert!(d.unified.contains("-bbb"));
}

#[test]
fn first_changed_line_correct() {
    let old = "line1\nline2\nline3\nline4\n";
    let new = "line1\nline2\nchanged\nline4\n";
    let d = unified_diff(old, new, "test.rs");
    assert_eq!(d.first_changed_line, Some(3));
}

#[test]
fn no_changes_empty_diff() {
    let content = "aaa\nbbb\n";
    let d = unified_diff(content, content, "test.rs");
    assert_eq!(d.added_lines, 0);
    assert_eq!(d.removed_lines, 0);
    assert_eq!(d.first_changed_line, None);
}

#[test]
fn diff_header_format() {
    let d = unified_diff("a\n", "b\n", "foo.rs");
    assert!(d.unified.starts_with("--- a/foo.rs\n+++ b/foo.rs\n"));
}
