use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::cluster::ClusterService;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::Impact;
use crate::kernel::OpType;

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
        "Dispatch a subtask to a remote bendclaw node by node_id. Returns a dispatch_id for tracking. Use cluster_nodes first to discover available nodes."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "The node_id (instance_id) of the target node from cluster_nodes"
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

        // Resolve node_id to endpoint from trusted peer cache
        let endpoint = match self.service.resolve_endpoint(node_id) {
            Ok(ep) => ep,
            Err(e) => return Ok(ToolResult::error(format!("{e}"))),
        };

        let user_id: &str = &ctx.user_id;
        let parent_run_id = Some(ctx.run_id.as_ref());
        match self
            .dispatch_table
            .dispatch(node_id, &endpoint, agent_id, task, user_id, parent_run_id)
            .await
        {
            Ok(dispatch_id) => {
                let result = json!({ "dispatch_id": dispatch_id });
                Ok(ToolResult::ok(result.to_string()))
            }
            Err(e) => Ok(ToolResult::error(format!("Dispatch failed: {e}"))),
        }
    }
}
