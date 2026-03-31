//! memory_search tool — FTS search over shared memory.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::memory::MemoryService;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::Impact;
use crate::kernel::OpType;

pub struct MemorySearchTool {
    memory: Arc<MemoryService>,
}

impl MemorySearchTool {
    pub fn new(memory: Arc<MemoryService>) -> Self {
        Self { memory }
    }
}

impl OperationClassifier for MemorySearchTool {
    fn op_type(&self) -> OpType {
        OpType::MemorySearch
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("?");
        format!("memory search: {query}")
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        ToolId::MemorySearch.as_str()
    }

    fn description(&self) -> &str {
        "Search long-term memory for relevant facts, preferences, and decisions. \
         Use before answering questions about prior work or user preferences."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default: 5)",
                    "default": 5
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
            _ => return Ok(ToolResult::error("Missing 'query' parameter")),
        };
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as u32;

        match self
            .memory
            .search(query, &ctx.user_id, &ctx.agent_id, limit)
            .await
        {
            Ok(results) if results.is_empty() => Ok(ToolResult::ok("No memories found.")),
            Ok(results) => {
                let items: Vec<serde_json::Value> = results
                    .iter()
                    .map(|r| {
                        json!({
                            "key": r.key,
                            "content": r.content,
                            "scope": format!("{}", r.scope),
                            "score": r.score,
                        })
                    })
                    .collect();
                Ok(ToolResult::ok(
                    serde_json::to_string_pretty(&items).unwrap_or_default(),
                ))
            }
            Err(e) => Ok(ToolResult::error(format!("Memory search failed: {e}"))),
        }
    }
}
