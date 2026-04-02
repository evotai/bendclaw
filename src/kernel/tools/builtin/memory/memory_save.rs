//! memory_save tool — persist a fact to shared memory.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::memory::MemoryScope;
use crate::kernel::memory::MemoryService;
use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::kernel::Impact;
use crate::kernel::OpType;

pub struct MemorySaveTool {
    memory: Arc<MemoryService>,
}

impl MemorySaveTool {
    pub fn new(memory: Arc<MemoryService>) -> Self {
        Self { memory }
    }
}

impl OperationClassifier for MemorySaveTool {
    fn op_type(&self) -> OpType {
        OpType::MemorySave
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("?");
        format!("memory save: {key}")
    }
}

#[async_trait]
impl Tool for MemorySaveTool {
    fn name(&self) -> &str {
        ToolId::MemorySave.as_str()
    }

    fn description(&self) -> &str {
        "Save a fact, preference, or decision to long-term memory. \
         Use scope 'shared' for facts useful to all agents, 'agent' for context-specific facts."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Short identifier for this memory (e.g. 'user_timezone')"
                },
                "content": {
                    "type": "string",
                    "description": "The fact or information to remember"
                },
                "scope": {
                    "type": "string",
                    "enum": ["agent", "shared"],
                    "description": "Visibility: 'agent' (this agent only) or 'shared' (all agents). Default: 'agent'",
                    "default": "agent"
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
        let key = match args.get("key").and_then(|v| v.as_str()) {
            Some(k) if !k.is_empty() => k,
            _ => return Ok(ToolResult::error("Missing 'key' parameter")),
        };
        let content = match args.get("content").and_then(|v| v.as_str()) {
            Some(c) if !c.is_empty() => c,
            _ => return Ok(ToolResult::error("Missing 'content' parameter")),
        };
        let scope = match args.get("scope").and_then(|v| v.as_str()) {
            Some("shared") => MemoryScope::Shared,
            _ => MemoryScope::Agent,
        };

        match self
            .memory
            .save(&ctx.user_id, &ctx.agent_id, key, content, scope)
            .await
        {
            Ok(()) => Ok(ToolResult::ok(format!(
                "Saved memory '{key}' (scope: {scope})"
            ))),
            Err(e) => Ok(ToolResult::error(format!("Failed to save memory: {e}"))),
        }
    }
}
