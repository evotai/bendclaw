//! Tests for built-in tools.

use base64::Engine;
use bendengine::tools::edit::EditFileTool;
use bendengine::tools::list::ListFilesTool;
use bendengine::tools::*;
use bendengine::types::*;
use tokio_util::sync::CancellationToken;

/// Helper to build a ToolContext for tests.
fn ctx(name: &str) -> ToolContext {
    ToolContext {
        tool_call_id: "t1".into(),
        tool_name: name.into(),
        cancel: CancellationToken::new(),
        on_update: None,
        on_progress: None,
    }
}

fn ctx_with_cancel(name: &str, cancel: CancellationToken) -> ToolContext {
    ToolContext {
        tool_call_id: "t1".into(),
        tool_name: name.into(),
        cancel,
        on_update: None,
        on_progress: None,
    }
}

#[tokio::test]
async fn test_bash_echo() {
    let tool = BashTool::new();
    let result = tool
        .execute(serde_json::json!({"command": "echo hello"}), ctx("bash"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("hello"));
    assert!(text.contains("Exit code: 0"));
}

#[tokio::test]
async fn test_bash_failure() {
    // Non-zero exit codes return Ok with exit code in output (for LLM self-correction)
    let tool = BashTool::new();
    let result = tool
        .execute(serde_json::json!({"command": "false"}), ctx("bash"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("Exit code: 1"));
}

#[tokio::test]
async fn test_bash_deny_pattern() {
    let tool = BashTool::new();
    let result = tool
        .execute(serde_json::json!({"command": "rm -rf /"}), ctx("bash"))
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("blocked"));
}

#[tokio::test]
async fn test_bash_timeout() {
    let tool = BashTool::new().with_timeout(std::time::Duration::from_millis(100));
    let result = tool
        .execute(serde_json::json!({"command": "sleep 10"}), ctx("bash"))
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("timed out"));
}

#[tokio::test]
async fn test_bash_cancel() {
    let tool = BashTool::new();
    let cancel = CancellationToken::new();
    cancel.cancel();

    let result = tool
        .execute(
            serde_json::json!({"command": "echo should not run"}),
            ctx_with_cancel("bash", cancel),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_read_write_file() {
    let tmp = std::env::temp_dir().join("yoagent-test-rw.txt");
    let path = tmp.to_str().unwrap();

    // Write
    let write_tool = WriteFileTool::new();
    let result = write_tool
        .execute(
            serde_json::json!({"path": path, "content": "hello from yoagent"}),
            ctx("write_file"),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("Wrote"));

    // Read
    let read_tool = ReadFileTool::new();
    let result = read_tool
        .execute(serde_json::json!({"path": path}), ctx("read_file"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("hello from yoagent"));

    // Cleanup
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_read_file_with_offset_limit() {
    let tmp = std::env::temp_dir().join("yoagent-test-lines.txt");
    let path = tmp.to_str().unwrap();

    let content = (1..=20)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&tmp, &content).unwrap();

    let tool = ReadFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": path, "offset": 5, "limit": 3}),
            ctx("read_file"),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("line 5"));
    assert!(text.contains("line 7"));
    assert!(!text.contains("line 8"));

    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_read_file_not_found() {
    let tool = ReadFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": "/nonexistent/file.txt"}),
            ctx("read_file"),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_write_creates_directories() {
    let tmp = std::env::temp_dir().join("yoagent-test-nested/deep/dir/file.txt");
    let path = tmp.to_str().unwrap();

    let tool = WriteFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": path, "content": "nested!"}),
            ctx("write_file"),
        )
        .await;

    assert!(result.is_ok());
    assert!(tmp.exists());

    // Cleanup
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("yoagent-test-nested"));
}

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

// --- Edit tool tests ---

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
    assert!(text.contains("Replaced"));
    let content = std::fs::read_to_string(&tmp).unwrap();
    assert!(content.contains("goodbye"));
    let _ = std::fs::remove_file(tmp);
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

#[tokio::test]
async fn test_read_file_line_numbers() {
    let tmp = std::env::temp_dir().join("yoagent-test-lineno2.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "first\nsecond\nthird\n").unwrap();
    let tool = ReadFileTool::new();
    let result = tool
        .execute(serde_json::json!({"path": path}), ctx("read_file"))
        .await
        .unwrap();
    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("   1 | first"));
    assert!(text.contains("   2 | second"));
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_bash_blocked_command() {
    let tool = BashTool::new();
    let result = tool
        .execute(serde_json::json!({"command": "rm -rf /"}), ctx("bash"))
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("blocked"));
}

#[tokio::test]
async fn test_default_tools_complete() {
    let tools = bendengine::tools::default_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 7);
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"edit_file"));
    assert!(names.contains(&"list_files"));
    assert!(names.contains(&"web_fetch"));
}

#[tokio::test]
async fn test_base_tools_complete() {
    let tools = bendengine::tools::base_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 7);
    assert!(names.contains(&"bash"));
}

// --- Image support tests ---

#[tokio::test]
async fn test_read_image_file() {
    // Minimal valid PNG (1x1 pixel, transparent)
    let png_bytes: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, // 8-bit RGB
        0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, // IDAT chunk
        0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC,
        0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, // IEND chunk
        0xAE, 0x42, 0x60, 0x82,
    ];

    let tmp = std::env::temp_dir().join("yoagent-test-image.png");
    std::fs::write(&tmp, &png_bytes).unwrap();

    let tool = ReadFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": tmp.to_str().unwrap()}),
            ctx("read_file"),
        )
        .await
        .unwrap();

    match &result.content[0] {
        Content::Image { data, mime_type } => {
            assert_eq!(mime_type, "image/png");
            assert!(!data.is_empty());
            // Verify round-trip: decode should match original bytes
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap();
            assert_eq!(decoded, png_bytes);
        }
        _ => panic!("expected Content::Image"),
    }

    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_read_jpeg_file() {
    let tmp = std::env::temp_dir().join("yoagent-test-image.jpg");
    std::fs::write(&tmp, b"fake-jpeg-data").unwrap();

    let tool = ReadFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": tmp.to_str().unwrap()}),
            ctx("read_file"),
        )
        .await
        .unwrap();

    match &result.content[0] {
        Content::Image { mime_type, .. } => {
            assert_eq!(mime_type, "image/jpeg");
        }
        _ => panic!("expected Content::Image for .jpg"),
    }

    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_read_text_file_unchanged() {
    // Non-image files should still return Content::Text
    let tmp = std::env::temp_dir().join("yoagent-test-notimage.txt");
    std::fs::write(&tmp, "just text").unwrap();

    let tool = ReadFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": tmp.to_str().unwrap()}),
            ctx("read_file"),
        )
        .await
        .unwrap();

    match &result.content[0] {
        Content::Text { text } => {
            assert!(text.contains("just text"));
        }
        _ => panic!("expected Content::Text for .txt file"),
    }

    let _ = std::fs::remove_file(tmp);
}

// --- Web fetch tool tests ---

#[tokio::test]
async fn test_web_fetch_missing_url() {
    let tool = bendengine::tools::web_fetch::WebFetchTool::new();
    let result = tool.execute(serde_json::json!({}), ctx("web_fetch")).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("url"));
}

#[tokio::test]
async fn test_web_fetch_success() {
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/hello"))
        .respond_with(ResponseTemplate::new(200).set_body_string("hello from mock"))
        .mount(&server)
        .await;

    let tool = bendengine::tools::web_fetch::WebFetchTool::new();
    let url = format!("{}/hello", server.uri());
    let result = tool
        .execute(serde_json::json!({"url": url}), ctx("web_fetch"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("hello from mock"));
}

#[tokio::test]
async fn test_web_fetch_with_headers() {
    use wiremock::matchers::header;
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_string("authenticated"))
        .mount(&server)
        .await;

    let tool = bendengine::tools::web_fetch::WebFetchTool::new();
    let url = format!("{}/auth", server.uri());
    let result = tool
        .execute(
            serde_json::json!({
                "url": url,
                "headers": { "Authorization": "Bearer test-token" }
            }),
            ctx("web_fetch"),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("authenticated"));
}

#[tokio::test]
async fn test_web_fetch_http_error() {
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/notfound"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let tool = bendengine::tools::web_fetch::WebFetchTool::new();
    let url = format!("{}/notfound", server.uri());
    let result = tool
        .execute(serde_json::json!({"url": url}), ctx("web_fetch"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("404"));
}

#[tokio::test]
async fn test_web_fetch_cancel() {
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("slow")
                .set_delay(std::time::Duration::from_secs(10)),
        )
        .mount(&server)
        .await;

    let tool = bendengine::tools::web_fetch::WebFetchTool::new();
    let cancel = CancellationToken::new();
    cancel.cancel();

    let url = format!("{}/slow", server.uri());
    let result = tool
        .execute(
            serde_json::json!({"url": url}),
            ctx_with_cancel("web_fetch", cancel),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_web_fetch_html_to_markdown() {
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let html = r#"<html><head><title>Test Page</title></head><body>
    <article>
    <h1>Hello</h1>
    <p>This is a paragraph with enough content for readability to extract it properly.
    It needs to be substantial enough to pass the content length threshold that the
    readability library uses to determine if content is worth extracting.</p>
    <p>Here is another paragraph to add more weight to the article body so the
    extraction algorithm considers this a real article worth converting.</p>
    </article>
    </body></html>"#;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/page"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html, "text/html; charset=utf-8"))
        .mount(&server)
        .await;

    let tool = bendengine::tools::web_fetch::WebFetchTool::new();
    let url = format!("{}/page", server.uri());
    let result = tool
        .execute(serde_json::json!({"url": url}), ctx("web_fetch"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    // Should be converted to markdown, not raw HTML
    assert!(!text.contains("<html>"));
    assert!(!text.contains("<p>"));
    // readability extracts the title from <title>, and the body from <article>
    assert!(text.contains("Test Page"));
    assert!(text.contains("paragraph"));
}
