use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use super::MemoryBackend;
use crate::base::Result;
use crate::kernel::agent_store::memory_store::SearchOpts;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;
use crate::observability::log::slog;

/// Search memories by query.
pub struct MemorySearchTool {
    storage: Arc<dyn MemoryBackend>,
}

impl MemorySearchTool {
    pub fn new(storage: Arc<dyn MemoryBackend>) -> Self {
        Self { storage }
    }
}

impl OperationClassifier for MemorySearchTool {
    fn op_type(&self) -> OpType {
        OpType::MemorySearch
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        ToolId::MemorySearch.as_str()
    }

    fn description(&self) -> &str {
        "Search memories by semantic similarity or keywords. Returns relevant memories with scores. \
         Call at the start of a task to retrieve prior user preferences or facts."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "include_tenant": {
                    "type": "boolean",
                    "default": true,
                    "description": "Include tenant-shared memories in results"
                },
                "max_results": {
                    "type": "integer",
                    "default": 10,
                    "description": "Maximum number of results"
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
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let include_tenant = args
            .get("include_tenant")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32;

        if query.is_empty() {
            return Ok(ToolResult::error("query is required"));
        }

        let opts = SearchOpts {
            max_results,
            include_shared: include_tenant,
            session_id: None,
            min_score: 0.0,
        };

        match self.storage.search(query, &ctx.user_id, opts).await {
            Ok(results) => {
                slog!(debug, "memory", "completed", query, results = results.len(),);
                if results.is_empty() {
                    return Ok(ToolResult::ok("No memories found."));
                }
                let mut output = String::new();
                for r in &results {
                    output.push_str(&format!(
                        "[{}] {} (score: {:.2})\n{}\n\n",
                        r.scope, r.key, r.score, r.content
                    ));
                }
                Ok(ToolResult::ok(output.trim()))
            }
            Err(e) => {
                slog!(warn, "memory", "failed", query, error = %e,);
                Ok(ToolResult::error(format!("Search failed: {e}")))
            }
        }
    }
}
