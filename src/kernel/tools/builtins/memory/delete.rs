use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;

/// Delete a memory by ID.
pub struct MemoryDeleteTool {
    storage: Arc<AgentStore>,
}

impl MemoryDeleteTool {
    pub fn new(storage: Arc<AgentStore>) -> Self {
        Self { storage }
    }
}

impl OperationClassifier for MemoryDeleteTool {
    fn op_type(&self) -> OpType {
        OpType::MemoryDelete
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for MemoryDeleteTool {
    fn name(&self) -> &str {
        ToolId::MemoryDelete.as_str()
    }

    fn description(&self) -> &str {
        "Delete a memory entry by its ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Memory ID to delete"
                }
            },
            "required": ["id"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");

        if id.is_empty() {
            return Ok(ToolResult::error("id is required"));
        }

        match self.storage.memory_delete(&ctx.user_id, id).await {
            Ok(()) => {
                tracing::info!(id, "memory deleted");
                Ok(ToolResult::ok(format!("Memory '{}' deleted.", id)))
            }
            Err(e) => {
                tracing::warn!(id, error = %e, "memory delete failed");
                Ok(ToolResult::error(format!("Failed to delete memory: {e}")))
            }
        }
    }
}
