//! Unified tool definition — pure metadata, no execution dependencies.
//!
//! Every tool the engine can call (builtin or skill) is described by a `ToolDefinition`.
//! Prompt rendering and LLM API calls consume this type.
//! Execution routing is handled separately via `ToolTarget` in the bindings map.

use crate::kernel::OpType;
use crate::llm::tool::ToolSchema;

/// Pure tool metadata — serializable, cacheable, no execution dependencies.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    /// Tool name as seen by the LLM.
    pub name: String,
    /// Human-readable description for the LLM.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: serde_json::Value,
    /// Operation type for tracking and diagnostics.
    pub op_type: OpType,
}

impl ToolDefinition {
    /// Create from a builtin `Tool` trait object (extracts metadata only).
    pub fn from_builtin(tool: &dyn crate::kernel::tools::tool_contract::Tool) -> Self {
        Self {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            input_schema: tool.parameters_schema(),
            op_type: tool.op_type(),
        }
    }

    /// Create from a skill's metadata.
    pub fn from_skill(name: String, description: String, input_schema: serde_json::Value) -> Self {
        Self {
            name,
            description,
            input_schema,
            op_type: OpType::SkillRun,
        }
    }

    /// Convert to the LLM-facing `ToolSchema` for API calls.
    pub fn to_tool_schema(&self) -> ToolSchema {
        ToolSchema::new(&self.name, &self.description, self.input_schema.clone())
    }
}
