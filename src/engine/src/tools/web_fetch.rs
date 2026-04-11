//! Web fetch tool — fetch content from a URL with timeout and size limits.
//!
//! Uses reqwest for HTTP requests. For HTML pages with sparse extracted text
//! (e.g. SPA / JS-rendered pages), automatically falls back to headless Chrome
//! if a browser is available on the system. HTML is converted to plain text
//! via html2text for consistent, simple output.

use std::time::Duration;

use async_trait::async_trait;

use crate::types::*;

const MAX_RESPONSE_SIZE: usize = 512 * 1024; // 512KB
const TIMEOUT_SECS: u64 = 30;
const BROWSER_RENDER_WAIT_SECS: u64 = 5;
const BROWSER_POLL_INTERVAL_MS: u64 = 300;
const BROWSER_TIMEOUT_SECS: u64 = 15;
const HTML_TEXT_WIDTH: usize = 100;

/// Minimum extracted text length to consider the reqwest result usable.
/// Below this threshold, browser fallback is attempted for HTML pages.
const BROWSER_FALLBACK_THRESHOLD: usize = 200;

/// Fetch content from a URL. Returns the response body as text.
pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentTool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn label(&self) -> &str {
        "Fetch URL"
    }

    fn description(&self) -> &str {
        "Fetches content from a URL. Returns the response body as text with a 512KB size limit.\n\
         \n\
         Use this tool to retrieve web pages, API responses, or any HTTP-accessible content.\n\
         Supports custom HTTP headers for authenticated requests.\n\
         \n\
         When you don't know the exact URL, search first using:\n\
         https://html.duckduckgo.com/html/?q=YOUR+SEARCH+TERMS\n\
         This returns search results with links you can then fetch directly.\n\
         \n\
         Parameters:\n\
         - url (required): The URL to fetch\n\
         - headers (optional): A JSON object of HTTP headers to include in the request"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers as key-value pairs",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'url' parameter".into()))?;

        let has_custom_headers = params
            .get("headers")
            .and_then(|h| h.as_object())
            .is_some_and(|h| !h.is_empty());

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .map_err(|e| ToolError::Failed(format!("Failed to create HTTP client: {e}")))?;

        let mut req = client.get(url);

        if let Some(headers) = params.get("headers").and_then(|h| h.as_object()) {
            for (key, value) in headers {
                if let Some(val) = value.as_str() {
                    req = req.header(key, val);
                }
            }
        }

        let cancel = ctx.cancel;

        let response = tokio::select! {
            _ = cancel.cancelled() => {
                return Err(ToolError::Cancelled);
            }
            result = req.send() => {
                result.map_err(|e| ToolError::Failed(format!("Fetch failed: {e}")))?
            }
        };

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if status >= 400 {
            let body = response
                .text()
                .await
                .map_err(|e| ToolError::Failed(format!("Failed to read response body: {e}")))?;
            return Ok(ToolResult {
                content: vec![Content::Text {
                    text: format!("HTTP {status} error fetching {url}"),
                }],
                details: serde_json::json!({ "status": status, "error": true, "body": body }),
            });
        }

        let body = response
            .text()
            .await
            .map_err(|e| ToolError::Failed(format!("Failed to read response body: {e}")))?;

        if !content_type.contains("text/html") {
            let text = truncate_text(body);
            return Ok(ToolResult {
                content: vec![Content::Text { text }],
                details: serde_json::json!({ "status": status, "renderer": "reqwest" }),
            });
        }

        let reqwest_text = html_to_text(&body);

        if !should_try_browser_fallback(&reqwest_text, has_custom_headers) {
            let text = truncate_text(reqwest_text);
            return Ok(ToolResult {
                content: vec![Content::Text { text }],
                details: serde_json::json!({ "status": status, "renderer": "reqwest" }),
            });
        }

        if let Some(ref progress) = ctx.on_progress {
            progress("Rendering page with browser...".into());
        }

        let url_owned = url.to_string();
        let browser_result = tokio::time::timeout(
            Duration::from_secs(BROWSER_TIMEOUT_SECS),
            tokio::task::spawn_blocking(move || browser_fetch_html(&url_owned)),
        )
        .await;

        if let Ok(Ok(Ok(rendered_html))) = browser_result {
            let text = truncate_text(html_to_text(&rendered_html));
            return Ok(ToolResult {
                content: vec![Content::Text { text }],
                details: serde_json::json!({ "status": status, "renderer": "browser_fallback" }),
            });
        }

        let text = truncate_text(reqwest_text);
        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::json!({ "status": status, "renderer": "reqwest" }),
        })
    }
}

/// Determine whether to attempt browser fallback for an HTML page.
///
/// Returns `true` when extracted text is too short to be useful
/// and no custom headers were provided (browser cannot replicate custom headers).
pub fn should_try_browser_fallback(extracted_text: &str, has_custom_headers: bool) -> bool {
    if has_custom_headers {
        return false;
    }
    extracted_text.trim().len() < BROWSER_FALLBACK_THRESHOLD
}

/// Render a page using headless Chrome and return the rendered HTML.
///
/// This is a blocking function — call via `spawn_blocking`.
/// Returns `Err` if Chrome is not installed or any step fails.
fn browser_fetch_html(url: &str) -> Result<String, String> {
    use headless_chrome::Browser;

    let browser = Browser::default().map_err(|e| format!("Browser launch failed: {e}"))?;
    let tab = browser
        .new_tab()
        .map_err(|e| format!("Failed to open tab: {e}"))?;
    tab.navigate_to(url)
        .map_err(|e| format!("Navigation failed: {e}"))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(BROWSER_RENDER_WAIT_SECS);
    let mut last_len: usize = 0;
    let mut stable_count: u8 = 0;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(BROWSER_POLL_INTERVAL_MS));
        let len = tab
            .evaluate("document.body ? document.body.innerText.length : 0", false)
            .ok()
            .and_then(|r| r.value.as_ref().and_then(|v| v.as_u64()))
            .unwrap_or(0) as usize;
        if len > 0 && len == last_len {
            stable_count += 1;
            if stable_count >= 3 {
                break;
            }
        } else {
            stable_count = 0;
        }
        last_len = len;
    }

    tab.get_content()
        .map_err(|e| format!("Failed to get content: {e}"))
}

/// Convert HTML to plain text.
fn html_to_text(html: &str) -> String {
    match html2text::from_read(html.as_bytes(), HTML_TEXT_WIDTH) {
        Ok(text) => text,
        Err(_) => html.to_string(),
    }
}

/// Truncate large text responses to the tool limit.
fn truncate_text(mut text: String) -> String {
    if text.len() > MAX_RESPONSE_SIZE {
        text.truncate(MAX_RESPONSE_SIZE);
        text.push_str("\n... (response truncated at 512KB)");
    }
    text
}
