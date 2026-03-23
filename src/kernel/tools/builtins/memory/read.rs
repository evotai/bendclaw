use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use super::MemoryBackend;
use crate::base::Result;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;
use crate::observability::log::slog;

/// Read a specific memory by key.
pub struct MemoryReadTool {
    storage: Arc<dyn MemoryBackend>,
}

impl MemoryReadTool {
    pub fn new(storage: Arc<dyn MemoryBackend>) -> Self {
        Self { storage }
    }
}

impl OperationClassifier for MemoryReadTool {
    fn op_type(&self) -> OpType {
        OpType::MemoryRead
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for MemoryReadTool {
    fn name(&self) -> &str {
        ToolId::MemoryRead.as_str()
    }

    fn description(&self) -> &str {
        "Read a specific memory entry by its key. Use memory_search if key is unknown."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Memory key to read"
                }
            },
            "required": ["key"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");

        if key.is_empty() {
            return Ok(ToolResult::error("key is required"));
        }

        match self.storage.get(&ctx.user_id, key).await {
            Ok(Some(entry)) => {
                slog!(debug, "memory", "completed", key, scope = %entry.scope,);
                Ok(ToolResult::ok(format!(
                    "[{}] {}\n\n{}",
                    entry.scope, entry.key, entry.content
                )))
            }
            Ok(None) => Ok(ToolResult::ok(format!("Memory '{}' not found.", key))),
            Err(e) => {
                slog!(warn, "memory", "failed", key, error = %e,);
                Ok(ToolResult::error(format!("Failed to read memory: {e}")))
            }
        }
    }
}
