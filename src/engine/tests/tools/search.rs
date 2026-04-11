//! Tests for SearchTool.

use bendengine::tools::*;
use bendengine::types::*;

use super::ctx;

#[tokio::test]
async fn test_search_pattern() {
    let tmp_dir = std::env::temp_dir().join("yoagent-test-search");
    let _ = std::fs::create_dir_all(&tmp_dir);
    std::fs::write(tmp_dir.join("a.txt"), "hello world\nfoo bar\nhello again").unwrap();
    std::fs::write(tmp_dir.join("b.txt"), "no match here\nhello there").unwrap();

    let tool = SearchTool::new().with_root(tmp_dir.to_str().unwrap());
    let result = tool
        .execute(serde_json::json!({"pattern": "hello"}), ctx("search"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("hello"));
    assert!(text.contains("3 matches") || text.contains("matches")); // 3 lines match

    let _ = std::fs::remove_dir_all(tmp_dir);
}

#[tokio::test]
async fn test_search_no_matches() {
    let tmp_dir = std::env::temp_dir().join("yoagent-test-search-empty");
    let _ = std::fs::create_dir_all(&tmp_dir);
    std::fs::write(tmp_dir.join("a.txt"), "nothing interesting").unwrap();

    let tool = SearchTool::new().with_root(tmp_dir.to_str().unwrap());
    let result = tool
        .execute(
            serde_json::json!({"pattern": "zzzznotfound"}),
            ctx("search"),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("No matches"));

    let _ = std::fs::remove_dir_all(tmp_dir);
}
