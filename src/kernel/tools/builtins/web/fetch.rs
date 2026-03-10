use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

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
        let url = Self::extract_url(args);
        if url.len() > 120 {
            format!("{}...", &url[..117])
        } else {
            url.to_string()
        }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        ToolId::WebFetch.as_str()
    }

    fn description(&self) -> &str {
        "Fetch the contents of a URL and return the response body as text."
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
                tracing::warn!(url, error = %e, "web_fetch failed");
                return Ok(ToolResult::error(format!("Request failed: {e}")));
            }
        };

        let status = resp.status();
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to read response body: {e}"
                )));
            }
        };

        let text = String::from_utf8_lossy(&body).to_string();
        tracing::info!(url, status = %status, body_len = body.len(), "web_fetch succeeded");

        if status.is_success() {
            Ok(ToolResult::ok(text))
        } else {
            Ok(ToolResult::error(format!("HTTP {status}: {text}")))
        }
    }
}
