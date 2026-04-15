//! Tests for the disallow mechanism on WriteFileTool and EditFileTool.

use evotengine::tools::edit::EditFileTool;
use evotengine::tools::file::WriteFileTool;
use evotengine::types::*;
use tokio_util::sync::CancellationToken;

fn ctx(name: &str) -> ToolContext {
    ToolContext {
        tool_call_id: "t1".into(),
        tool_name: name.into(),
        cancel: CancellationToken::new(),
        on_update: None,
        on_progress: None,
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
    }
}

// ---------------------------------------------------------------------------
// WriteFileTool
// ---------------------------------------------------------------------------

#[tokio::test]
async fn write_file_disallowed_returns_error() {
    let tool = WriteFileTool::new().disallow("not allowed right now");
    let result = tool
        .execute(
            serde_json::json!({"path": "/tmp/should-not-exist.txt", "content": "x"}),
            ctx("write_file"),
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not allowed right now"));
}

#[tokio::test]
async fn write_file_disallowed_does_not_write() {
    let path = std::env::temp_dir().join("disallow-test-no-write.txt");
    let _ = std::fs::remove_file(&path);

    let tool = WriteFileTool::new().disallow("blocked");
    let _ = tool
        .execute(
            serde_json::json!({"path": path.to_str().unwrap(), "content": "should not appear"}),
            ctx("write_file"),
        )
        .await;

    assert!(!path.exists(), "file should not have been created");
}

#[tokio::test]
async fn write_file_normal_still_works() {
    let path = std::env::temp_dir().join("disallow-test-normal-write.txt");
    let _ = std::fs::remove_file(&path);

    let tool = WriteFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": path.to_str().unwrap(), "content": "hello"}),
            ctx("write_file"),
        )
        .await;

    assert!(result.is_ok());
    assert!(path.exists());
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// EditFileTool
// ---------------------------------------------------------------------------

#[tokio::test]
async fn edit_file_disallowed_returns_error() {
    let tool = EditFileTool::new().disallow("editing is off");
    let result = tool
        .execute(
            serde_json::json!({"path": "/tmp/x.txt", "old_text": "a", "new_text": "b"}),
            ctx("edit_file"),
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("editing is off"));
}

#[tokio::test]
async fn edit_file_disallowed_does_not_modify() {
    let path = std::env::temp_dir().join("disallow-test-no-edit.txt");
    std::fs::write(&path, "original content").unwrap();

    let tool = EditFileTool::new().disallow("blocked");
    let _ = tool
        .execute(
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_text": "original",
                "new_text": "modified"
            }),
            ctx("edit_file"),
        )
        .await;

    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content, "original content");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn edit_file_normal_still_works() {
    let path = std::env::temp_dir().join("disallow-test-normal-edit.txt");
    std::fs::write(&path, "aaa bbb ccc").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_text": "bbb",
                "new_text": "zzz"
            }),
            ctx("edit_file"),
        )
        .await;

    assert!(result.is_ok());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("zzz"));
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// Planning-style disallow: write and edit visible but blocked
// ---------------------------------------------------------------------------

#[test]
fn disallowed_tools_still_have_names() {
    let write = WriteFileTool::new().disallow("blocked");
    let edit = EditFileTool::new().disallow("blocked");
    assert_eq!(write.name(), "write_file");
    assert_eq!(edit.name(), "edit_file");
}

#[tokio::test]
async fn disallowed_write_returns_message() {
    let tool = WriteFileTool::new().disallow("plan mode");
    let result = tool
        .execute(
            serde_json::json!({"path": "/tmp/x.txt", "content": "x"}),
            ctx("write_file"),
        )
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("plan mode"));
}

#[tokio::test]
async fn disallowed_edit_returns_message() {
    let tool = EditFileTool::new().disallow("plan mode");
    let result = tool
        .execute(
            serde_json::json!({"path": "/tmp/x.txt", "old_text": "a", "new_text": "b"}),
            ctx("edit_file"),
        )
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("plan mode"));
}

#[tokio::test]
async fn read_file_still_works_alongside_disallowed() {
    let path = std::env::temp_dir().join("disallow-test-plan-read.txt");
    std::fs::write(&path, "readable").unwrap();

    let tool = evotengine::tools::ReadFileTool::default();
    let result = tool
        .execute(
            serde_json::json!({"path": path.to_str().unwrap()}),
            ctx("read_file"),
        )
        .await;

    assert!(result.is_ok());
    let _ = std::fs::remove_file(&path);
}
