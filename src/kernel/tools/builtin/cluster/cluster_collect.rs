use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::kernel::Impact;
use crate::kernel::OpType;
use crate::observability::log::slog;

/// Collect results from previously dispatched subtasks.
pub struct ClusterCollectTool {
    dispatch_table: Arc<DispatchTable>,
}

impl ClusterCollectTool {
    pub fn new(dispatch_table: Arc<DispatchTable>) -> Self {
        Self { dispatch_table }
    }
}

impl OperationClassifier for ClusterCollectTool {
    fn op_type(&self) -> OpType {
        OpType::ClusterCollect
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let count = args
            .get("dispatch_ids")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        format!("collect {count} dispatches")
    }
}

#[async_trait]
impl Tool for ClusterCollectTool {
    fn name(&self) -> &str {
        ToolId::ClusterCollect.as_str()
    }

    fn description(&self) -> &str {
        "Collect results from previously dispatched subtasks. Polls until all are complete or timeout."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "dispatch_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of dispatch_id values to collect results for"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Maximum seconds to wait for results (default 120)"
                }
            },
            "required": ["dispatch_ids"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let dispatch_ids: Vec<String> = match args.get("dispatch_ids").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            None => {
                return Ok(ToolResult::error(
                    "Missing or invalid 'dispatch_ids' parameter",
                ))
            }
        };

        if dispatch_ids.is_empty() {
            return Ok(ToolResult::error("'dispatch_ids' must not be empty"));
        }

        let timeout_secs = args
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(120);
        let timeout = Duration::from_secs(timeout_secs);
        let started = std::time::Instant::now();

        match self.dispatch_table.collect(&dispatch_ids, timeout).await {
            Ok(entries) => {
                let json =
                    serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".to_string());
                let pending: Vec<String> = entries
                    .iter()
                    .filter(|entry| {
                        !matches!(entry.status.as_str(), "COMPLETED" | "ERROR" | "CANCELLED")
                    })
                    .map(|entry| format!("{}:{}", entry.dispatch_id, entry.status))
                    .collect();
                if !pending.is_empty() {
                    slog!(warn, "cluster", "pending",
                        user_id = %ctx.user_id,
                        agent_id = %ctx.agent_id,
                        run_id = %ctx.run_id,
                        pending = ?pending,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                    );
                }
                Ok(ToolResult::ok(json))
            }
            Err(e) => {
                slog!(warn, "cluster", "failed",
                    user_id = %ctx.user_id,
                    agent_id = %ctx.agent_id,
                    run_id = %ctx.run_id,
                    dispatch_count = dispatch_ids.len(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    error = %e,
                );
                Ok(ToolResult::error(format!("Collect failed: {e}")))
            }
        }
    }
}
