use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use crate::base::truncate_chars_with_ellipsis;
use crate::base::truncate_with_notice;
use crate::base::Result;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::Impact;
use crate::kernel::OpType;

/// Fetch the contents of a URL.
pub struct WebFetchTool;

/// Max bytes for successful response bodies (~25K tokens).
const MAX_FETCH_BODY: usize = 100_000;
/// Max bytes for error response bodies.
const MAX_ERROR_BODY: usize = 4_000;

impl WebFetchTool {
    fn extract_url(args: &serde_json::Value) -> &str {
        args.get("url").and_then(|v| v.as_str()).unwrap_or("")
    }
}

impl OperationClassifier for WebFetchTool {
    fn op_type(&self) -> OpType {
        OpType::WebFetch
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        truncate_chars_with_ellipsis(Self::extract_url(args), 120)
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        ToolId::WebFetch.as_str()
    }

    fn description(&self) -> &str {
        "Fetch a URL and return its content. HTML pages are converted to readable markdown. \
         Also works for JSON/text APIs."
    }

    fn hint(&self) -> &str {
        "fetch a URL and return content as markdown"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let url = match args.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return Ok(ToolResult::error("Missing 'url' parameter")),
        };

        // Reject non-HTTP(S) schemes
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult::error(
                "Only http:// and https:// URLs are supported",
            ));
        }

        // Validate URL syntax
        if let Err(e) = reqwest::Url::parse(url) {
            return Ok(ToolResult::error(format!("Invalid URL: {e}")));
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("bendclaw/0.1")
            .build()
            .unwrap_or_default();

        let resp = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(stage = "web_fetch", status = "failed", url, error = %e, "web_fetch failed");
                return Ok(ToolResult::error(format!("Request failed: {e}")));
            }
        };

        let status = resp.status();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to read response body: {e}"
                )));
            }
        };

        let text = String::from_utf8_lossy(&body);
        tracing::info!(stage = "web_fetch", status = "completed", url, http_status = %status, body_len = body.len(), "web_fetch completed");

        if status.is_success() {
            let output = if content_type.contains("text/html") {
                match super::html::html_to_markdown(&text) {
                    Some(md) => md,
                    None => text.to_string(),
                }
            } else {
                text.to_string()
            };
            Ok(ToolResult::ok(truncate_with_notice(
                &output,
                MAX_FETCH_BODY,
            )))
        } else {
            let truncated = truncate_with_notice(&text, MAX_ERROR_BODY);
            Ok(ToolResult::error(format!("HTTP {status}: {truncated}")))
        }
    }
}
