use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::cluster::ClusterService;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::Impact;
use crate::kernel::OpType;

/// Discover available peer nodes in the cluster.
pub struct ClusterNodesTool {
    service: Arc<ClusterService>,
}

impl ClusterNodesTool {
    pub fn new(service: Arc<ClusterService>) -> Self {
        Self { service }
    }
}

impl OperationClassifier for ClusterNodesTool {
    fn op_type(&self) -> OpType {
        OpType::ClusterNodes
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }

    fn summarize(&self, _args: &serde_json::Value) -> String {
        "discover cluster nodes".to_string()
    }
}

#[async_trait]
impl Tool for ClusterNodesTool {
    fn name(&self) -> &str {
        ToolId::ClusterNodes.as_str()
    }

    fn description(&self) -> &str {
        "Discover available peer nodes in the cluster. Returns a list of nodes with their node_id, endpoint, load, and status. Refreshes the peer cache."
    }

    fn hint(&self) -> &str {
        "list cluster peer nodes"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute_with_context(
        &self,
        _args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        tracing::info!(
            stage = "cluster_nodes",
            status = "started",
            user_id = %ctx.user_id,
            agent_id = %ctx.agent_id,
            run_id = %ctx.run_id,
            "cluster_nodes started"
        );
        match self.service.refresh_peers().await {
            Ok(nodes) => {
                let json =
                    serde_json::to_string_pretty(&nodes).unwrap_or_else(|_| "[]".to_string());
                tracing::info!(
                    stage = "cluster_nodes",
                    status = "completed",
                    user_id = %ctx.user_id,
                    agent_id = %ctx.agent_id,
                    run_id = %ctx.run_id,
                    node_count = nodes.len(),
                    "cluster_nodes completed"
                );
                Ok(ToolResult::ok(json))
            }
            Err(e) => {
                tracing::warn!(
                    stage = "cluster_nodes",
                    status = "failed",
                    user_id = %ctx.user_id,
                    agent_id = %ctx.agent_id,
                    run_id = %ctx.run_id,
                    error = %e,
                    "cluster_nodes failed"
                );
                Ok(ToolResult::error(format!("Failed to discover nodes: {e}")))
            }
        }
    }
}
