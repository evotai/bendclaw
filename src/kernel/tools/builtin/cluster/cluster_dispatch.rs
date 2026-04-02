use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::cluster::ClusterService;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::kernel::Impact;
use crate::kernel::OpType;
use crate::observability::log::slog;

/// Dispatch a subtask to a remote bendclaw node.
pub struct ClusterDispatchTool {
    service: Arc<ClusterService>,
    dispatch_table: Arc<DispatchTable>,
}

impl ClusterDispatchTool {
    pub fn new(service: Arc<ClusterService>, dispatch_table: Arc<DispatchTable>) -> Self {
        Self {
            service,
            dispatch_table,
        }
    }
}

impl OperationClassifier for ClusterDispatchTool {
    fn op_type(&self) -> OpType {
        OpType::ClusterDispatch
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::High)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let node_id = args.get("node_id").and_then(|v| v.as_str()).unwrap_or("");
        format!("dispatch to {node_id}")
    }
}

#[async_trait]
impl Tool for ClusterDispatchTool {
    fn name(&self) -> &str {
        ToolId::ClusterDispatch.as_str()
    }

    fn description(&self) -> &str {
        "Dispatch a subtask to a remote bendclaw node by node_id. Returns a dispatch_id for tracking. \
         Use cluster_nodes first to discover available nodes. No nested dispatch."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "The node_id of the target node from cluster_nodes"
                },
                "agent_id": {
                    "type": "string",
                    "description": "The agent ID to run on the remote node"
                },
                "task": {
                    "type": "string",
                    "description": "The task description / input for the remote agent"
                }
            },
            "required": ["node_id", "agent_id", "task"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        // Single-layer fanout: reject dispatch from an already-dispatched run.
        if ctx.is_dispatched {
            return Ok(ToolResult::error(
                "nested dispatch is not allowed, only one level of fanout is supported",
            ));
        }

        let node_id = match args.get("node_id").and_then(|v| v.as_str()) {
            Some(n) if !n.is_empty() => n,
            _ => return Ok(ToolResult::error("Missing or empty 'node_id' parameter")),
        };
        let agent_id = match args.get("agent_id").and_then(|v| v.as_str()) {
            Some(a) if !a.is_empty() => a,
            _ => return Ok(ToolResult::error("Missing or empty 'agent_id' parameter")),
        };
        let task = match args.get("task").and_then(|v| v.as_str()) {
            Some(t) if !t.is_empty() => t,
            _ => return Ok(ToolResult::error("Missing or empty 'task' parameter")),
        };
        let started = std::time::Instant::now();

        // Resolve node_id to endpoint from trusted peer cache
        let endpoint = match self.service.resolve_endpoint(node_id) {
            Ok(ep) => ep,
            Err(e) => {
                slog!(warn, "cluster", "resolve_failed",
                    user_id = %ctx.user_id,
                    agent_id = %ctx.agent_id,
                    run_id = %ctx.run_id,
                    node_id,
                    error = %e,
                );
                return Ok(ToolResult::error(format!("{e}")));
            }
        };

        let user_id: &str = &ctx.user_id;
        let parent_run_id = Some(ctx.run_id.as_ref());
        let trace_id = Some(ctx.trace_id.as_ref());
        match self
            .dispatch_table
            .dispatch(
                node_id,
                &endpoint,
                agent_id,
                task,
                user_id,
                parent_run_id,
                trace_id,
                Some(self.service.node_id()), // origin_node_id
            )
            .await
        {
            Ok(dispatch_id) => {
                let result = json!({ "dispatch_id": dispatch_id });

                Ok(ToolResult::ok(result.to_string()))
            }
            Err(e) => {
                slog!(warn, "cluster", "failed",
                    user_id = %ctx.user_id,
                    agent_id = %ctx.agent_id,
                    run_id = %ctx.run_id,
                    node_id,
                    endpoint,
                    remote_agent_id = agent_id,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    error = %e,
                );
                Ok(ToolResult::error(format!("Dispatch failed: {e}")))
            }
        }
    }
}
