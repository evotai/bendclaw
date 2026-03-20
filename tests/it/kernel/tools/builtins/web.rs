use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bendclaw::kernel::tools::web::WebFetchTool;
use bendclaw::kernel::tools::web::WebSearchTool;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use serde_json::json;

use crate::mocks::context::test_tool_context;

#[tokio::test]
async fn web_search_missing_query_returns_error() -> Result<(), Box<dyn std::error::Error>> {
    let tool = WebSearchTool::default();
    let ctx = test_tool_context();

    let result = tool.execute_with_context(json!({}), &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("Missing or empty 'query'")));
    Ok(())
}

#[tokio::test]
async fn web_search_missing_api_key_returns_error_without_db_lookup(
) -> Result<(), Box<dyn std::error::Error>> {
    let tool = WebSearchTool::default();
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(json!({"query": "databend"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("No BRAVE_API_KEY variable configured")));
    Ok(())
}

async fn start_server(status: axum::http::StatusCode, body: &'static str) -> String {
    async fn ok() -> (axum::http::StatusCode, &'static str) {
        (axum::http::StatusCode::OK, "hello from server")
    }

    async fn fail() -> (axum::http::StatusCode, &'static str) {
        (axum::http::StatusCode::BAD_GATEWAY, "upstream failed")
    }

    let app = match status {
        axum::http::StatusCode::OK => Router::new().route("/", get(ok)),
        _ => Router::new().route("/", get(fail)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local test server");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve local test server");
    });
    let _ = body;
    format!("http://{addr}/")
}

#[tokio::test]
async fn web_fetch_success_returns_body() -> Result<(), Box<dyn std::error::Error>> {
    let tool = WebFetchTool;
    let ctx = test_tool_context();
    let url = start_server(axum::http::StatusCode::OK, "hello from server").await;

    let result = tool
        .execute_with_context(json!({ "url": url }), &ctx)
        .await?;

    assert!(result.success);
    assert_eq!(result.output, "hello from server");
    Ok(())
}

#[tokio::test]
async fn web_fetch_http_error_returns_error() -> Result<(), Box<dyn std::error::Error>> {
    let tool = WebFetchTool;
    let ctx = test_tool_context();
    let url = start_server(axum::http::StatusCode::BAD_GATEWAY, "upstream failed").await;

    let result = tool
        .execute_with_context(json!({ "url": url }), &ctx)
        .await?;

    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|error| error.contains("502") && error.contains("upstream failed")));
    Ok(())
}

#[tokio::test]
async fn web_fetch_rejects_non_http_scheme() -> Result<(), Box<dyn std::error::Error>> {
    let tool = WebFetchTool;
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(json!({ "url": "file:///tmp/demo" }), &ctx)
        .await?;

    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|error| error.contains("Only http:// and https:// URLs are supported")));
    Ok(())
}

#[test]
fn web_fetch_summarize_long_multibyte_url_safely() {
    let tool = WebFetchTool;
    let url = format!("https://example.com/{}", "路径".repeat(100));
    let result = tool.summarize(&json!({ "url": url }));
    assert!(result.ends_with("..."));
    assert_eq!(result.chars().count(), 120);
}

#[tokio::test]
async fn web_fetch_truncates_large_success_body() -> Result<(), Box<dyn std::error::Error>> {
    async fn large_ok() -> (axum::http::StatusCode, String) {
        (axum::http::StatusCode::OK, "x".repeat(200_000))
    }

    let app = Router::new().route("/", get(large_ok));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let tool = WebFetchTool;
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({ "url": format!("http://{addr}/") }), &ctx)
        .await?;

    assert!(result.success);
    assert!(result.output.len() < 200_000);
    assert!(result.output.contains("[truncated:"));
    Ok(())
}

#[tokio::test]
async fn web_fetch_truncates_large_error_body() -> Result<(), Box<dyn std::error::Error>> {
    async fn large_err() -> (axum::http::StatusCode, String) {
        (
            axum::http::StatusCode::NOT_FOUND,
            "e".repeat(50_000),
        )
    }

    let app = Router::new().route("/", get(large_err));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let tool = WebFetchTool;
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({ "url": format!("http://{addr}/") }), &ctx)
        .await?;

    assert!(!result.success);
    let err = result.error.as_deref().unwrap_or("");
    assert!(err.contains("404"));
    assert!(err.contains("[truncated:"));
    assert!(err.len() < 50_000);
    Ok(())
}

#[tokio::test]
async fn web_search_success_formats_results_and_caps_count(
) -> Result<(), Box<dyn std::error::Error>> {
    async fn search() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "web": {
                "results": [
                    {
                        "title": "Databend",
                        "url": "https://databend.com",
                        "description": "Cloud data warehouse"
                    }
                ]
            }
        }))
    }

    let app = Router::new().route("/search", get(search));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve search test server");
    });

    let tool = WebSearchTool::new(format!("http://{addr}/search"));
    let _ws_dir = std::env::temp_dir().join(format!("bendclaw-web-search-{}", ulid::Ulid::new()));
    let workspace = bendclaw::kernel::session::workspace::Workspace::from_variable_records(
        _ws_dir.clone(),
        _ws_dir,
        vec!["PATH".into(), "HOME".into()],
        vec![bendclaw::storage::VariableRecord {
            id: "var-brave".into(),
            key: "BRAVE_API_KEY".into(),
            value: "token".into(),
            secret: false,
            revoked: false,
            user_id: String::new(),
            scope: String::new(),
            created_by: String::new(),
            last_used_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        }],
        std::time::Duration::from_secs(5),
        1_048_576,
        std::sync::Arc::new(bendclaw::kernel::session::workspace::SandboxResolver),
    );
    let ctx = bendclaw::kernel::tools::ToolContext {
        user_id: "u1".into(),
        session_id: "s1".into(),
        agent_id: "a1".into(),
        run_id: "r-test".into(),
        trace_id: "t-test".into(),
        workspace: std::sync::Arc::new(workspace),
        pool: crate::mocks::context::dummy_pool(),
        is_dispatched: false,
        runtime: bendclaw::kernel::tools::ToolRuntime {
            event_tx: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            cli_agent_state: bendclaw::kernel::tools::cli_agent::new_shared_state(),
            tool_call_id: None,
        },
        tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
    };

    let result = tool
        .execute_with_context(json!({"query": "databend", "count": 99}), &ctx)
        .await?;

    assert!(result.success);
    assert!(result.output.contains("Databend"));
    assert!(result.output.contains("https://databend.com"));
    assert!(result.output.contains("Cloud data warehouse"));
    Ok(())
}

#[tokio::test]
async fn web_fetch_converts_html_to_markdown() -> Result<(), Box<dyn std::error::Error>> {
    async fn html_page() -> impl IntoResponse {
        (
            axum::http::StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<html><head><title>Title</title></head><body><article><h1>Title</h1><p>Hello <strong>world</strong>.</p></article></body></html>",
        )
    }

    let app = Router::new().route("/", get(html_page));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve"); });

    let tool = WebFetchTool;
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({ "url": format!("http://{addr}/") }), &ctx)
        .await?;

    assert!(result.success);
    // Should contain markdown, not raw HTML tags
    assert!(result.output.contains("Title"));
    assert!(result.output.contains("world"));
    assert!(!result.output.contains("<article>"));
    Ok(())
}

#[tokio::test]
async fn web_fetch_json_not_converted() -> Result<(), Box<dyn std::error::Error>> {
    async fn json_api() -> impl IntoResponse {
        (
            axum::http::StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            r#"{"key":"value"}"#,
        )
    }

    let app = Router::new().route("/", get(json_api));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve"); });

    let tool = WebFetchTool;
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({ "url": format!("http://{addr}/") }), &ctx)
        .await?;

    assert!(result.success);
    assert_eq!(result.output, r#"{"key":"value"}"#);
    Ok(())
}

#[tokio::test]
async fn web_fetch_html_conversion_failure_falls_back() -> Result<(), Box<dyn std::error::Error>> {
    // Serve a content-type of text/html but with content that readability can't extract
    async fn bad_html() -> impl IntoResponse {
        (
            axum::http::StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/html")],
            "",
        )
    }

    let app = Router::new().route("/", get(bad_html));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve"); });

    let tool = WebFetchTool;
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({ "url": format!("http://{addr}/") }), &ctx)
        .await?;

    // Should succeed with fallback to raw text (empty in this case)
    assert!(result.success);
    Ok(())
}
