//! Tests for WebFetchTool.

use bendengine::types::*;
use tokio_util::sync::CancellationToken;

use super::ctx;
use super::ctx_with_cancel;

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
