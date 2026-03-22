use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::recall::RecallStore;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;

pub struct KnowledgeSearchTool {
    store: Arc<RecallStore>,
}

impl KnowledgeSearchTool {
    pub fn new(store: Arc<RecallStore>) -> Self {
        Self { store }
    }
}

impl OperationClassifier for KnowledgeSearchTool {
    fn op_type(&self) -> OpType {
        OpType::KnowledgeSearch
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for KnowledgeSearchTool {
    fn name(&self) -> &str {
        ToolId::KnowledgeSearch.as_str()
    }

    fn description(&self) -> &str {
        "Search the agent's knowledge base for known sources, files, schemas, and discoveries."
    }

    fn hint(&self) -> &str {
        "search knowledge base"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
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
        _ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32;

        if query.is_empty() {
            return Ok(ToolResult::error("query is required"));
        }

        match self.store.knowledge().search(query, max_results).await {
            Ok(results) => {
                tracing::info!(
                    stage = "knowledge_search",
                    status = "completed",
                    query,
                    results = results.len(),
                    "knowledge_search completed"
                );
                if results.is_empty() {
                    return Ok(ToolResult::ok("No knowledge entries found."));
                }
                let mut output = String::new();
                for r in &results {
                    output.push_str(&format!(
                        "[{}] {}: {}\n  locator: {}\n  status: {} | confidence: {:.2}\n\n",
                        r.kind, r.title, r.summary, r.locator, r.status, r.confidence
                    ));
                }
                Ok(ToolResult::ok(output.trim()))
            }
            Err(e) => {
                tracing::warn!(stage = "knowledge_search", status = "failed", query, error = %e, "knowledge_search failed");
                Ok(ToolResult::error(format!("Search failed: {e}")))
            }
        }
    }
}
