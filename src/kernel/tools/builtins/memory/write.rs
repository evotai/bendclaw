use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use ulid::Ulid;

use super::MemoryBackend;
use crate::base::Result;
use crate::kernel::agent_store::memory_store::MemoryEntry;
use crate::kernel::agent_store::memory_store::MemoryScope;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;

/// Write a memory entry.
pub struct MemoryWriteTool {
    storage: Arc<dyn MemoryBackend>,
}

impl MemoryWriteTool {
    pub fn new(storage: Arc<dyn MemoryBackend>) -> Self {
        Self { storage }
    }
}

impl OperationClassifier for MemoryWriteTool {
    fn op_type(&self) -> OpType {
        OpType::MemoryWrite
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        ToolId::MemoryWrite.as_str()
    }

    fn description(&self) -> &str {
        "Write a memory entry. Use scope 'user' for personal, 'shared' for tenant-shared, 'session' for temporary."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Memory identifier (e.g., 'project-bendclaw', 'preference-theme')"
                },
                "content": {
                    "type": "string",
                    "description": "Memory content in Markdown format"
                },
                "scope": {
                    "type": "string",
                    "enum": ["user", "shared", "session"],
                    "default": "user",
                    "description": "Memory scope: user (personal), shared (tenant), session (temporary)"
                }
            },
            "required": ["key", "content"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let scope_str = args.get("scope").and_then(|v| v.as_str()).unwrap_or("user");
        let normalized_scope = match scope_str {
            "tenant" => "shared",
            other => other,
        };

        if key.is_empty() {
            return Ok(ToolResult::error("key is required"));
        }
        if content.is_empty() {
            return Ok(ToolResult::error("content is required"));
        }

        let scope = match normalized_scope {
            "shared" => MemoryScope::Shared,
            "session" => MemoryScope::Session,
            _ => MemoryScope::User,
        };

        let entry = MemoryEntry {
            id: Ulid::new().to_string().to_lowercase(),
            user_id: ctx.user_id.to_string(),
            scope,
            session_id: if scope == MemoryScope::Session {
                Some(ctx.session_id.to_string())
            } else {
                None
            },
            key: key.to_string(),
            content: content.to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        };

        let op = crate::kernel::writer::tool_op::ToolWriteOp::MemoryWrite {
            storage: self.storage.clone(),
            user_id: ctx.user_id.to_string(),
            entry,
        };
        ctx.tool_writer.send(op);

        Ok(ToolResult::ok(format!(
            "Memory '{}' written (scope: {})",
            key, normalized_scope
        )))
    }
}
