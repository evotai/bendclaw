//! Tests for ListFilesTool.

use bendengine::tools::list::ListFilesTool;
use bendengine::types::*;

use super::ctx;

#[tokio::test]
async fn test_list_files_tool() {
    let tmp_dir = std::env::temp_dir().join("yoagent-test-list2");
    let _ = std::fs::create_dir_all(tmp_dir.join("sub"));
    std::fs::write(tmp_dir.join("a.rs"), "").unwrap();
    std::fs::write(tmp_dir.join("sub/c.rs"), "").unwrap();
    let tool = ListFilesTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": tmp_dir.to_str().unwrap()}),
            ctx("list_files"),
        )
        .await
        .unwrap();
    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("a.rs"));
    let _ = std::fs::remove_dir_all(tmp_dir);
}
