use std::time::Duration;

use async_trait::async_trait;

use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::kernel::Impact;
use crate::kernel::OpType;
use crate::observability::log::slog;
use crate::types::truncate_chars_with_ellipsis;
use crate::types::truncate_with_notice;
use crate::types::Result;

const DESCRIPTION: &str = "\
Fetch a URL and return its content. HTML pages are converted to readable markdown. \
Also works for JSON/text APIs.\n\
\n\
Usage:\n\
- Use this tool when you need to retrieve and analyze web content.\n\
- Use after web_search to read a specific URL — do not guess URLs from memory.\n\
- The URL must be a fully-formed valid URL.\n\
- HTTP URLs will be automatically upgraded to HTTPS.\n\
- This tool is read-only and does not modify any files.\n\
- Results may be summarized if the content is very large.\n\
- Includes a self-cleaning 15-minute cache for faster responses when repeatedly \
accessing the same URL.\n\
- When a URL redirects to a different host, the tool will inform you and provide \
the redirect URL. You should then make a new request with the redirect URL.\n\
- For GitHub URLs, prefer using the gh CLI via shell instead \
(e.g., gh pr view, gh issue view, gh api).\n\
- You MUST include the relevant data in your response — quote specific facts, \
numbers, or passages.";

fn schema() -> serde_json::Value {
    serde_json::json!({
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
        DESCRIPTION
    }

    fn parameters_schema(&self) -> serde_json::Value {
        schema()
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
                slog!(warn, "web", "failed", url, error = %e,);
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

        if status.is_success() {
            let output = if content_type.contains("text/html") {
                match crate::kernel::tools::web::html::html_to_markdown(&text) {
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
