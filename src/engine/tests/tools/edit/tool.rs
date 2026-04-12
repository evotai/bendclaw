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
