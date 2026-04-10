//! Web fetch tool — fetch content from a URL with timeout and size limits.
//!
//! Uses reqwest for HTTP requests. For HTML pages where readability extraction
//! fails (e.g. SPA / JS-rendered pages), automatically falls back to headless
//! Chrome rendering if a browser is available on the system.

use std::io::Cursor;
use std::time::Duration;

use async_trait::async_trait;

use crate::types::*;

const MAX_RESPONSE_SIZE: usize = 512 * 1024; // 512KB
const TIMEOUT_SECS: u64 = 30;
const BROWSER_RENDER_WAIT_SECS: u64 = 5;
const BROWSER_POLL_INTERVAL_MS: u64 = 300;
const BROWSER_TIMEOUT_SECS: u64 = 15;

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

        // --- Non-HTML: return as-is ---
        if !content_type.contains("text/html") {
            let mut text = body;
            if text.len() > MAX_RESPONSE_SIZE {
                text.truncate(MAX_RESPONSE_SIZE);
                text.push_str("\n... (response truncated at 512KB)");
            }
            return Ok(ToolResult {
                content: vec![Content::Text { text }],
                details: serde_json::json!({ "status": status, "renderer": "reqwest" }),
            });
        }

        // --- HTML: try readability extraction ---
        let extracted = html_to_markdown(&body, url);

        if !should_try_browser_fallback(extracted.as_deref(), has_custom_headers) {
            let mut text = extracted.unwrap_or(body);
            if text.len() > MAX_RESPONSE_SIZE {
                text.truncate(MAX_RESPONSE_SIZE);
                text.push_str("\n... (response truncated at 512KB)");
            }
            return Ok(ToolResult {
                content: vec![Content::Text { text }],
                details: serde_json::json!({ "status": status, "renderer": "reqwest" }),
            });
        }

        // --- Browser fallback for SPA / JS-rendered pages ---
        tracing::debug!(
            url,
            extracted_len = extracted.as_deref().map(|t| t.len()),
            "Attempting browser fallback for SPA page"
        );

        if let Some(ref progress) = ctx.on_progress {
            progress("Rendering page with browser...".into());
        }

        let url_owned = url.to_string();
        let browser_result = tokio::time::timeout(
            Duration::from_secs(BROWSER_TIMEOUT_SECS),
            tokio::task::spawn_blocking(move || browser_fetch(&url_owned)),
        )
        .await;

        match &browser_result {
            Ok(Ok(Ok(r))) => {
                tracing::debug!(
                    url,
                    rendered_len = r.html.len(),
                    visible_text_len = r.visible_text.len(),
                    "Browser fallback succeeded"
                );
            }
            Ok(Ok(Err(e))) => {
                tracing::debug!(url, error = %e, "Browser fetch failed");
            }
            Ok(Err(e)) => {
                tracing::debug!(url, error = %e, "Browser task panicked");
            }
            Err(_) => {
                tracing::debug!(url, "Browser fallback timed out");
            }
        }

        if let Ok(Ok(Ok(result))) = browser_result {
            // Prefer JS-extracted visible text; fall back to readability on rendered HTML
            let mut text = if result.visible_text.len() >= BROWSER_FALLBACK_THRESHOLD {
                result.visible_text
            } else {
                html_to_markdown(&result.html, url).unwrap_or(result.visible_text)
            };
            if text.len() > MAX_RESPONSE_SIZE {
                text.truncate(MAX_RESPONSE_SIZE);
                text.push_str("\n... (response truncated at 512KB)");
            }
            return Ok(ToolResult {
                content: vec![Content::Text { text }],
                details: serde_json::json!({ "status": status, "renderer": "browser_fallback" }),
            });
        }

        // Browser failed — return original reqwest result
        let mut text = extracted.unwrap_or(body);
        if text.len() > MAX_RESPONSE_SIZE {
            text.truncate(MAX_RESPONSE_SIZE);
            text.push_str("\n... (response truncated at 512KB)");
        }

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::json!({ "status": status, "renderer": "reqwest" }),
        })
    }
}

/// Determine whether to attempt browser fallback for an HTML page.
///
/// Returns `true` when readability extraction produced no usable content
/// and no custom headers were provided (browser cannot replicate custom headers).
pub fn should_try_browser_fallback(extracted_text: Option<&str>, has_custom_headers: bool) -> bool {
    if has_custom_headers {
        return false;
    }
    match extracted_text {
        None => true,
        Some(text) => text.len() < BROWSER_FALLBACK_THRESHOLD,
    }
}

/// Result from browser rendering — includes both HTML and extracted visible text.
struct BrowserFetchResult {
    /// The visible text content extracted via JS from the rendered page.
    visible_text: String,
    /// The full rendered HTML (fallback if text extraction is poor).
    html: String,
}

/// Render a page using headless Chrome and extract content.
///
/// This is a blocking function — call via `spawn_blocking`.
/// Returns `Err` if Chrome is not installed or any step fails.
fn browser_fetch(url: &str) -> Result<BrowserFetchResult, String> {
    use headless_chrome::Browser;

    let browser = Browser::default().map_err(|e| format!("Browser launch failed: {e}"))?;
    let tab = browser
        .new_tab()
        .map_err(|e| format!("Failed to open tab: {e}"))?;
    tab.navigate_to(url)
        .map_err(|e| format!("Navigation failed: {e}"))?;

    // Poll until page content stabilizes or timeout is reached.
    // This avoids both fixed sleeps (wasteful) and wait_until_navigated (unreliable for SPAs).
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
            if stable_count >= 2 {
                break;
            }
        } else {
            stable_count = 0;
        }
        last_len = len;
    }

    // Extract visible text via JS — works for any page including listing pages
    let js_result = tab
        .evaluate(
            r#"
            (() => {
                const title = document.title || '';
                const walker = document.createTreeWalker(
                    document.body,
                    NodeFilter.SHOW_TEXT,
                    {
                        acceptNode: (node) => {
                            const el = node.parentElement;
                            if (!el) return NodeFilter.FILTER_REJECT;
                            const tag = el.tagName;
                            if (tag === 'SCRIPT' || tag === 'STYLE' || tag === 'NOSCRIPT')
                                return NodeFilter.FILTER_REJECT;
                            const style = window.getComputedStyle(el);
                            if (style.display === 'none' || style.visibility === 'hidden')
                                return NodeFilter.FILTER_REJECT;
                            return NodeFilter.FILTER_ACCEPT;
                        }
                    }
                );
                const parts = [];
                while (walker.nextNode()) {
                    const text = walker.currentNode.textContent.trim();
                    if (text) parts.push(text);
                }
                return JSON.stringify({ title, text: parts.join('\n') });
            })()
            "#,
            false,
        )
        .map_err(|e| format!("JS evaluation failed: {e}"))?;

    let visible_text = js_result
        .value
        .as_ref()
        .and_then(|v| v.as_str())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .map(|obj| {
            let title = obj["title"].as_str().unwrap_or("");
            let text = obj["text"].as_str().unwrap_or("");
            if title.is_empty() {
                text.to_string()
            } else {
                format!("# {title}\n\n{text}")
            }
        })
        .unwrap_or_default();

    let html = tab
        .get_content()
        .map_err(|e| format!("Failed to get content: {e}"))?;

    Ok(BrowserFetchResult { visible_text, html })
}

/// Extract readable content from HTML and convert it to markdown.
///
/// Returns `None` if the input cannot be parsed or has no extractable content,
/// allowing the caller to fall back to the raw text.
fn html_to_markdown(html: &str, url: &str) -> Option<String> {
    let mut cursor = Cursor::new(html.as_bytes());
    let parsed_url = reqwest::Url::parse(url).ok()?;
    let article = readability::extractor::extract(&mut cursor, &parsed_url).ok()?;

    let md = htmd::convert(&article.content).ok()?;
    let trimmed = md.trim();
    if trimmed.is_empty() {
        return None;
    }

    let title = article.title.trim();
    if title.is_empty() {
        Some(trimmed.to_string())
    } else {
        Some(format!("# {title}\n\n{trimmed}"))
    }
}
