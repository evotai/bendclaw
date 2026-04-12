//! Tests for EditFileTool execute and preview.

use bendengine::tools::edit::EditFileTool;
use bendengine::types::*;

use super::super::ctx;

#[tokio::test]
async fn test_edit_file() {
    let tmp = std::env::temp_dir().join("yoagent-test-edit.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "old_text": "println!(\"hello\")",
                "new_text": "println!(\"goodbye\")"
            }),
            ctx("edit_file"),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("Updated"));

    // details should contain a diff field for REPL display
    let diff = result.details["diff"].as_str().unwrap();
    assert!(diff.contains("-    println!(\"hello\")"));
    assert!(diff.contains("+    println!(\"goodbye\")"));

    let content = std::fs::read_to_string(&tmp).unwrap();
    assert!(content.contains("goodbye"));
    let _ = std::fs::remove_file(tmp);
}

#[test]
fn test_edit_file_preview_command() {
    let tool = EditFileTool::new();
    let params =
        serde_json::json!({"path": "/tmp/foo.rs", "old_text": "old_code", "new_text": "new_code"});
    let cmd = tool.preview_command(&params).unwrap();
    assert!(cmd.starts_with("sed -i"));
    assert!(cmd.contains("/tmp/foo.rs"));
    assert!(cmd.contains("<old>/<new>"));
}

#[test]
fn test_edit_file_preview_command_missing_path() {
    let tool = EditFileTool::new();
    let params = serde_json::json!({"old_text": "a", "new_text": "b"});
    assert!(tool.preview_command(&params).is_none());
}

#[tokio::test]
async fn test_edit_file_no_match() {
    let tmp = std::env::temp_dir().join("yoagent-test-edit-nomatch.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "hello world\n").unwrap();
    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": path, "old_text": "nonexistent", "new_text": "bar"}),
            ctx("edit_file"),
        )
        .await;
    assert!(result.is_err());
    let _ = std::fs::remove_file(tmp);
}

// ---------------------------------------------------------------------------
// replace_all
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_replace_all_multiple_occurrences() {
    let tmp = std::env::temp_dir().join("yoagent-test-replace-all.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "foo bar foo baz foo\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "old_text": "foo",
                "new_text": "qux",
                "replace_all": true
            }),
            ctx("edit_file"),
        )
        .await
        .unwrap();

    let content = std::fs::read_to_string(&tmp).unwrap();
    assert_eq!(content, "qux bar qux baz qux\n");
    assert_eq!(result.details["replace_all"], true);
    assert_eq!(result.details["replacement_count"], 3);
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_replace_all_single_occurrence() {
    let tmp = std::env::temp_dir().join("yoagent-test-replace-all-single.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "hello world\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "old_text": "world",
                "new_text": "earth",
                "replace_all": true
            }),
            ctx("edit_file"),
        )
        .await
        .unwrap();

    let content = std::fs::read_to_string(&tmp).unwrap();
    assert!(content.contains("earth"));
    assert_eq!(result.details["replacement_count"], 1);
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_replace_all_not_found() {
    let tmp = std::env::temp_dir().join("yoagent-test-replace-all-notfound.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "hello world\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "old_text": "nonexistent",
                "new_text": "bar",
                "replace_all": true
            }),
            ctx("edit_file"),
        )
        .await;

    let err = result.unwrap_err().to_string();
    assert!(err.contains("not found"));
    assert!(err.contains("replace_all requires an exact match"));
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_replace_all_empty_old_text() {
    let tmp = std::env::temp_dir().join("yoagent-test-replace-all-empty.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "hello\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "old_text": "",
                "new_text": "bar",
                "replace_all": true
            }),
            ctx("edit_file"),
        )
        .await;

    let err = result.unwrap_err().to_string();
    assert!(err.contains("old_text must not be empty"));
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_not_unique_error_mentions_replace_all() {
    let tmp = std::env::temp_dir().join("yoagent-test-not-unique-hint.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "aaa\nbbb\naaa\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "old_text": "aaa",
                "new_text": "ccc"
            }),
            ctx("edit_file"),
        )
        .await;

    let err = result.unwrap_err().to_string();
    assert!(err.contains("2 locations"));
    assert!(err.contains("replace_all"));
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_replace_all_overlapping_pattern() {
    // Rust str::replace is non-overlapping: "aaaa".replace("aa","b") == "bb"
    let tmp = std::env::temp_dir().join("yoagent-test-replace-all-overlap.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "aaaa\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "old_text": "aa",
                "new_text": "b",
                "replace_all": true
            }),
            ctx("edit_file"),
        )
        .await
        .unwrap();

    let content = std::fs::read_to_string(&tmp).unwrap();
    assert_eq!(content, "bb\n");
    // str::matches counts non-overlapping matches = 2
    assert_eq!(result.details["replacement_count"], 2);
    let _ = std::fs::remove_file(tmp);
}

// ---------------------------------------------------------------------------
// preview_command with replace_all
// ---------------------------------------------------------------------------

#[test]
fn test_preview_command_replace_all() {
    let tool = EditFileTool::new();
    let params = serde_json::json!({
        "path": "/tmp/foo.rs",
        "old_text": "old",
        "new_text": "new",
        "replace_all": true
    });
    let cmd = tool.preview_command(&params).unwrap();
    assert!(cmd.contains("/g'"));
}

#[test]
fn test_preview_command_no_replace_all() {
    let tool = EditFileTool::new();
    let params = serde_json::json!({
        "path": "/tmp/foo.rs",
        "old_text": "old",
        "new_text": "new"
    });
    let cmd = tool.preview_command(&params).unwrap();
    assert!(!cmd.contains("/g'"));
    assert!(cmd.contains("<old>/<new>/'"));
}

// ---------------------------------------------------------------------------
// details fields
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_details_contains_replace_all_and_count() {
    let tmp = std::env::temp_dir().join("yoagent-test-details-fields.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "old_text": "println!(\"hello\")",
                "new_text": "println!(\"bye\")"
            }),
            ctx("edit_file"),
        )
        .await
        .unwrap();

    assert_eq!(result.details["replace_all"], false);
    assert_eq!(result.details["replacement_count"], 1);
    let _ = std::fs::remove_file(tmp);
}
