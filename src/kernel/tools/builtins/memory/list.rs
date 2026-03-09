use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::agent_store::memory_store::MemoryScope;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;

/// List all memories.
pub struct MemoryListTool {
    storage: Arc<AgentStore>,
}

impl MemoryListTool {
    pub fn new(storage: Arc<AgentStore>) -> Self {
        Self { storage }
    }
}

impl OperationClassifier for MemoryListTool {
    fn op_type(&self) -> OpType {
        OpType::MemoryList
    }

    fn summarize(&self, _args: &serde_json::Value) -> String {
        "list memories".to_string()
    }
}

#[async_trait]
impl Tool for MemoryListTool {
    fn name(&self) -> &str {
        ToolId::MemoryList.as_str()
    }

    fn description(&self) -> &str {
        "List all memories for the current user (including tenant-shared)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "default": 20,
                    "description": "Maximum number of memories to list"
                }
            },
            "required": []
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as u32;

        match self.storage.memory_list(&ctx.user_id, limit).await {
            Ok(entries) => {
                tracing::info!(count = entries.len(), "memory list completed");
                if entries.is_empty() {
                    return Ok(ToolResult::ok("No memories found."));
                }
                let mut output = String::new();
                for e in &entries {
                    let scope_str = match e.scope {
                        MemoryScope::Shared => "shared",
                        MemoryScope::Session => "session",
                        MemoryScope::User => "user",
                    };
                    output.push_str(&format!(
                        "[{}] {} ({})\n  ID: {}\n  Updated: {}\n\n",
                        scope_str, e.key, e.id, e.id, e.updated_at
                    ));
                }
                Ok(ToolResult::ok(output.trim()))
            }
            Err(e) => {
                tracing::warn!(error = %e, "memory list failed");
                Ok(ToolResult::error(format!("Failed to list memories: {e}")))
            }
        }
    }
}
