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
use crate::storage::dal::variable::VariableRepo;

/// Search the web using the Brave Search API.
pub struct WebSearchTool;

impl WebSearchTool {
    fn extract_query(args: &serde_json::Value) -> &str {
        args.get("query").and_then(|v| v.as_str()).unwrap_or("")
    }
}

impl OperationClassifier for WebSearchTool {
    fn op_type(&self) -> OpType {
        OpType::WebSearch
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        Self::extract_query(args).to_string()
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        ToolId::WebSearch.as_str()
    }

    fn description(&self) -> &str {
        "Search the web using the Brave Search API and return results."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of results to return (default 5, max 10)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let query = match args.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.is_empty() => q,
            _ => return Ok(ToolResult::error("Missing or empty 'query' parameter")),
        };

        let count = args
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(10) as u32;

        // Read BRAVE_API_KEY from variables (same pattern as ShellTool)
        let repo = VariableRepo::new(ctx.pool.clone());
        let variables = match repo.list_all().await {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult::error(format!("Failed to load variables: {e}")));
            }
        };

        let api_key = variables.iter().find(|v| v.key == "BRAVE_API_KEY");
        let api_key = match api_key {
            Some(v) => &v.value,
            None => {
                return Ok(ToolResult::error(
                    "No BRAVE_API_KEY variable configured. Add it via the variables API.",
                ));
            }
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("bendclaw/0.1")
            .build()
            .unwrap_or_default();

        let resp = match client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", api_key.as_str())
            .query(&[("q", query), ("count", &count.to_string())])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(query, error = %e, "web_search request failed");
                return Ok(ToolResult::error(format!("Search request failed: {e}")));
            }
        };

        let status = resp.status();
        let body: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to parse search response: {e}"
                )));
            }
        };

        if !status.is_success() {
            return Ok(ToolResult::error(format!(
                "Brave API HTTP {status}: {body}"
            )));
        }

        // Format results
        let results = body
            .get("web")
            .and_then(|w| w.get("results"))
            .and_then(|r| r.as_array());

        let output = match results {
            Some(items) => {
                let mut lines = Vec::new();
                for item in items {
                    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    let desc = item
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    lines.push(format!("{title}\n{url}\n{desc}"));
                }
                lines.join("\n\n")
            }
            None => "No results found.".to_string(),
        };

        tracing::info!(query, count, "web_search succeeded");
        Ok(ToolResult::ok(output))
    }
}
