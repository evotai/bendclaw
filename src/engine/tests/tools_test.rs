//! Tests for built-in tools.

use std::sync::Arc;

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
    assert!(cmd.contains("old_code"));
    assert!(cmd.contains("new_code"));
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

// --- Bash streaming / cleanup tests ---

#[tokio::test]
async fn test_bash_timeout_includes_partial_output() {
    let tool = BashTool::new().with_timeout(std::time::Duration::from_millis(500));
    let result = tool
        .execute(
            serde_json::json!({"command": "echo before_timeout; sleep 10"}),
            ctx("bash"),
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("timed out"));
    assert!(err.contains("before_timeout"));
}

#[tokio::test]
async fn test_bash_progress_emitted() {
    let progress_count = Arc::new(parking_lot::Mutex::new(0u32));
    let count_ref = progress_count.clone();

    let on_progress: Option<ProgressFn> = Some(Arc::new(move |_text: String| {
        *count_ref.lock() += 1;
    }));

    let cancel = CancellationToken::new();
    let tool_ctx = ToolContext {
        tool_call_id: "t1".into(),
        tool_name: "bash".into(),
        cancel,
        on_update: None,
        on_progress,
    };

    // Command runs ~4s, progress fires every 3s, so we should get at least 1
    let tool = BashTool::new();
    let _result = tool
        .execute(
            serde_json::json!({"command": "for i in 1 2 3 4; do echo $i; sleep 1; done"}),
            tool_ctx,
        )
        .await;

    let count = *progress_count.lock();
    assert!(
        count >= 1,
        "Expected at least 1 progress callback, got {count}"
    );
}

#[tokio::test]
async fn test_bash_update_emitted() {
    let updates = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let updates_ref = updates.clone();

    let on_update: Option<ToolUpdateFn> = Some(Arc::new(move |partial: ToolResult| {
        if let Some(Content::Text { text }) = partial.content.first() {
            updates_ref.lock().push(text.clone());
        }
    }));

    let cancel = CancellationToken::new();
    let tool_ctx = ToolContext {
        tool_call_id: "t1".into(),
        tool_name: "bash".into(),
        cancel,
        on_update,
        on_progress: None,
    };

    // Command runs ~4s with output, update fires every 2s
    let tool = BashTool::new();
    let _result = tool
        .execute(
            serde_json::json!({"command": "for i in 1 2 3 4; do echo line_$i; sleep 1; done"}),
            tool_ctx,
        )
        .await;

    let collected = updates.lock();
    assert!(
        !collected.is_empty(),
        "Expected at least 1 update callback with partial output"
    );
    // At least one update should contain some of our output
    assert!(
        collected.iter().any(|s| s.contains("line_")),
        "Expected partial output to contain 'line_', got: {collected:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_bash_timeout_no_hang() {
    // Verify that timeout + kill completes promptly, not hanging on zombie/pipe
    let start = std::time::Instant::now();
    let tool = BashTool::new().with_timeout(std::time::Duration::from_millis(200));
    let result = tool
        .execute(serde_json::json!({"command": "sleep 999"}), ctx("bash"))
        .await;

    assert!(result.is_err());
    let elapsed = start.elapsed();
    // Should complete well within 5 seconds (200ms timeout + kill + drain)
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "Bash timeout took too long: {elapsed:?}, expected < 5s"
    );
}

// --- Long line truncation tests ---

#[tokio::test]
async fn test_bash_long_line_truncated() {
    // A single line longer than 4096 bytes should be truncated
    let tool = BashTool::new();
    let long = "x".repeat(8000);
    let cmd = format!("printf '{long}'");
    let result = tool
        .execute(serde_json::json!({"command": cmd}), ctx("bash"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("bytes truncated"));
    // Should keep head and tail
    assert!(text.starts_with("Exit code: 0\nxx"));
    assert!(text.ends_with("xx"));
    // Total output should be much smaller than 8000
    assert!(text.len() < 6000);
}

#[tokio::test]
async fn test_bash_short_line_not_truncated() {
    let tool = BashTool::new();
    let short = "y".repeat(100);
    let cmd = format!("printf '{short}'");
    let result = tool
        .execute(serde_json::json!({"command": cmd}), ctx("bash"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(!text.contains("bytes truncated"));
    assert!(text.contains(&short));
}

#[tokio::test]
async fn test_bash_multiline_only_long_lines_truncated() {
    // Mix of short and long lines — only the long one gets truncated
    let tool = BashTool::new();
    let long = "z".repeat(8000);
    let cmd = format!("echo short_line; printf '{long}'; echo; echo another_short");
    let result = tool
        .execute(serde_json::json!({"command": cmd}), ctx("bash"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("short_line"));
    assert!(text.contains("another_short"));
    assert!(text.contains("bytes truncated"));
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
async fn test_web_fetch_html_to_text() {
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let html = r#"<html><head><title>Test Page</title></head><body>
    <article>
    <h1>Hello</h1>
    <p>This is a paragraph with enough content for text extraction to include it.</p>
    <p>Here is another paragraph to make the extracted text clearly longer.</p>
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
    assert!(!text.contains("<html>"));
    assert!(!text.contains("<p>"));
    assert!(text.contains("Test Page") || text.contains("Hello"));
    assert!(text.contains("paragraph"));
}

// --- Browser fallback decision tests ---

#[test]
fn test_should_try_browser_fallback_short_text() {
    use bendengine::tools::web_fetch::should_try_browser_fallback;
    assert!(should_try_browser_fallback("short", false));
}

#[test]
fn test_should_try_browser_fallback_sufficient_text() {
    use bendengine::tools::web_fetch::should_try_browser_fallback;
    let long_text = "x".repeat(200);
    assert!(!should_try_browser_fallback(&long_text, false));
}

#[test]
fn test_should_try_browser_fallback_with_custom_headers() {
    use bendengine::tools::web_fetch::should_try_browser_fallback;
    assert!(!should_try_browser_fallback("", true));
    assert!(!should_try_browser_fallback("short", true));
}

#[tokio::test]
async fn test_web_fetch_json_no_browser_fallback() {
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/data"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"key": "value"}"#)
                .insert_header("content-type", "application/json"),
        )
        .mount(&server)
        .await;

    let tool = bendengine::tools::web_fetch::WebFetchTool::new();
    let url = format!("{}/api/data", server.uri());
    let result = tool
        .execute(serde_json::json!({"url": url}), ctx("web_fetch"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("key"));
    assert!(text.contains("value"));
    assert_eq!(result.details["renderer"], "reqwest");
}

#[tokio::test]
async fn test_web_fetch_html_good_content_no_fallback() {
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let html = r#"<html><head><title>Test Page</title></head><body>
    <article>
    <h1>Hello</h1>
    <p>This is a paragraph with enough content for text extraction to include it, and it should be
    comfortably long enough that html2text produces a clearly useful body of text for the reqwest path.
    We want this content to exceed the browser fallback threshold without needing any JS rendering.</p>
    <p>Here is another paragraph to make the extracted text clearly longer, with additional descriptive
    content that simulates a normal article page and ensures the direct HTML-to-text conversion is sufficient.</p>
    <p>A third paragraph adds even more plain text so the output remains comfortably above the threshold
    and the tool should stay on the reqwest renderer instead of invoking browser fallback.</p>
    </article>
    </body></html>"#;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/good-page"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html, "text/html; charset=utf-8"))
        .mount(&server)
        .await;

    let tool = bendengine::tools::web_fetch::WebFetchTool::new();
    let url = format!("{}/good-page", server.uri());
    let result = tool
        .execute(serde_json::json!({"url": url}), ctx("web_fetch"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("paragraph"));
    assert_eq!(result.details["renderer"], "reqwest");
}

#[tokio::test]
async fn test_web_fetch_headers_skip_browser_fallback() {
    use wiremock::matchers::header;
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    let spa_html = r#"<html><head><title>App</title></head><body><div id="root"></div>
    <script src="/bundle.js"></script></body></html>"#;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/spa"))
        .and(header("Authorization", "Bearer token"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(spa_html, "text/html; charset=utf-8"))
        .mount(&server)
        .await;

    let tool = bendengine::tools::web_fetch::WebFetchTool::new();
    let url = format!("{}/spa", server.uri());
    let result = tool
        .execute(
            serde_json::json!({
                "url": url,
                "headers": { "Authorization": "Bearer token" }
            }),
            ctx("web_fetch"),
        )
        .await
        .unwrap();

    assert_eq!(result.details["renderer"], "reqwest");
}
