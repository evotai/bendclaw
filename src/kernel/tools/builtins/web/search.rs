use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use super::cache::WebCache;
use super::duckduckgo;
use crate::base::Result;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::Impact;
use crate::kernel::OpType;
use crate::observability::log::slog;

/// Which search backend to use.
#[derive(Clone, Debug, Default)]
pub enum SearchProvider {
    Brave,
    DuckDuckGo,
    /// Try Brave if API key is available, fall back to DuckDuckGo.
    #[default]
    Auto,
}

const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

/// Search the web using Brave (with API key) or DuckDuckGo (zero-config fallback).
#[derive(Clone)]
pub struct WebSearchTool {
    client: reqwest::Client,
    brave_base_url: String,
    provider: SearchProvider,
    cache: Arc<WebCache>,
}

impl WebSearchTool {
    pub fn new(brave_base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("bendclaw/0.1")
                .build()
                .unwrap_or_default(),
            brave_base_url: brave_base_url.into(),
            provider: SearchProvider::Auto,
            cache: Arc::new(WebCache::new(DEFAULT_CACHE_TTL)),
        }
    }

    pub fn with_provider(mut self, provider: SearchProvider) -> Self {
        self.provider = provider;
        self
    }

    pub fn with_cache(mut self, cache: Arc<WebCache>) -> Self {
        self.cache = cache;
        self
    }

    fn extract_query(args: &serde_json::Value) -> &str {
        args.get("query").and_then(|v| v.as_str()).unwrap_or("")
    }

    /// Resolve the Brave API key: workspace variable first, then env var.
    fn resolve_brave_key(ctx: &ToolContext) -> Option<BraveKey> {
        if let Some(v) = ctx.workspace.variable("BRAVE_API_KEY") {
            return Some(BraveKey {
                value: v.value.clone(),
                secret: v.secret,
                id: Some(v.id.clone()),
            });
        }
        std::env::var("BRAVE_API_KEY").ok().map(|value| BraveKey {
            value,
            secret: false,
            id: None,
        })
    }

    /// Execute a Brave search. Returns `Ok(output)` on success, `Err(msg)` on failure.
    async fn brave_search(
        &self,
        query: &str,
        count: u32,
        api_key: &str,
    ) -> std::result::Result<String, String> {
        let resp = self
            .client
            .get(&self.brave_base_url)
            .header("X-Subscription-Token", api_key)
            .query(&[("q", query), ("count", &count.to_string())])
            .send()
            .await
            .map_err(|e| format!("Brave search request failed: {e}"))?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Brave response: {e}"))?;

        if !status.is_success() {
            return Err(format!("Brave API HTTP {status}: {body}"));
        }

        let results = body
            .get("web")
            .and_then(|w| w.get("results"))
            .and_then(|r| r.as_array());

        match results {
            Some(items) if !items.is_empty() => {
                let lines: Vec<String> = items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
                        let desc = item
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        format!("{}. {title}\n{url}\n{desc}", i + 1)
                    })
                    .collect();
                Ok(format!(
                    "Found {} results:\n\n{}",
                    lines.len(),
                    lines.join("\n\n")
                ))
            }
            _ => Ok("No results found.".to_string()),
        }
    }
}

struct BraveKey {
    value: String,
    secret: bool,
    id: Option<String>,
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new("https://api.search.brave.com/res/v1/web/search")
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
        "Search the web for current information, news, documentation, or any topic. \
         Returns relevant results with titles, URLs, and descriptions. \
         Always search first — do not construct URLs from memory. \
         Be specific with queries for better results. \
         Only use web_fetch when you need full page content from a URL found in search results. \
         You MUST include the relevant data in your response — quote specific facts, numbers, or passages."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let year = chrono::Utc::now().format("%Y");
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": format!("The search query. Be specific and use keywords for better results. For example, use 'Rust async runtime tokio {year}' instead of 'tell me about async in Rust'.")
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

        // Check cache first
        let cache_key = WebCache::search_key(query, count);
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(ToolResult::ok(cached));
        }

        let brave_key = Self::resolve_brave_key(ctx);

        let output = match &self.provider {
            SearchProvider::Brave => {
                let key = match &brave_key {
                    Some(k) => k,
                    None => {
                        slog!(warn, "web", "no_api_key", provider = "brave",);
                        return Ok(ToolResult::error(
                            "No BRAVE_API_KEY variable configured. \
                             Add it via the variables API or set the BRAVE_API_KEY env var.",
                        ));
                    }
                };
                match self.brave_search(query, count, &key.value).await {
                    Ok(output) => output,
                    Err(e) => {
                        slog!(warn, "web", "failed", provider = "brave", query, error = %e,);
                        return Ok(ToolResult::error(e));
                    }
                }
            }
            SearchProvider::DuckDuckGo => {
                match duckduckgo::search(&self.client, query, count).await {
                    Ok(results) => duckduckgo::format_results(&results),
                    Err(e) => {
                        slog!(warn, "web", "failed", provider = "duckduckgo", query, error = %e,);
                        return Ok(ToolResult::error(e));
                    }
                }
            }
            SearchProvider::Auto => {
                if let Some(key) = &brave_key {
                    match self.brave_search(query, count, &key.value).await {
                        Ok(output) => output,
                        Err(e) => {
                            slog!(warn, "web", "fallback", provider = "brave->duckduckgo", query, error = %e,);
                            match duckduckgo::search(&self.client, query, count).await {
                                Ok(results) => duckduckgo::format_results(&results),
                                Err(e2) => {
                                    slog!(warn, "web", "failed", provider = "brave+duckduckgo", query, error = %e2,);
                                    return Ok(ToolResult::error(format!(
                                        "Brave: {e}; DuckDuckGo: {e2}"
                                    )));
                                }
                            }
                        }
                    }
                } else {
                    slog!(
                        info,
                        "web",
                        "no_api_key",
                        provider = "auto->duckduckgo",
                        query,
                    );
                    match duckduckgo::search(&self.client, query, count).await {
                        Ok(results) => duckduckgo::format_results(&results),
                        Err(e) => {
                            slog!(warn, "web", "failed", provider = "duckduckgo", query, error = %e,);
                            return Ok(ToolResult::error(e));
                        }
                    }
                }
            }
        };

        // Touch secret variable last-used timestamp
        if let Some(key) = brave_key {
            if key.secret {
                if let Some(id) = key.id {
                    let pool = ctx.pool.clone();
                    crate::base::spawn_fire_and_forget("variable_touch_last_used", async move {
                        let repo = crate::storage::dal::variable::VariableRepo::new(pool);
                        let _ = repo.touch_last_used(&id).await;
                    });
                }
            }
        }

        // Store in cache
        self.cache.insert(cache_key, output.clone());

        Ok(ToolResult::ok(output))
    }
}
