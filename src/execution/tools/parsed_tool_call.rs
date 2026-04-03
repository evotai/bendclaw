use super::tool_result::ToolCallResult;
use crate::llm::message::ToolCall;
use crate::tools::definition::tool_definition::ToolDefinition;
use crate::tools::definition::tool_target::ToolTarget;

/// Parsed tool call with resolved definition and dispatch target.
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub call: ToolCall,
    pub arguments: serde_json::Value,
    /// Resolved metadata. `None` if the tool name was not found.
    pub definition: Option<ToolDefinition>,
    /// Resolved dispatch target. `None` if the tool name was not found.
    pub target: Option<ToolTarget>,
}

impl ParsedToolCall {
    pub fn is_builtin(&self) -> bool {
        self.target.as_ref().is_some_and(|t| t.is_builtin())
    }

    pub fn is_skill(&self) -> bool {
        self.target.as_ref().is_some_and(|t| t.is_skill())
    }

    /// Label for diagnostics: "tool" or "skill".
    pub fn kind_str(&self) -> &'static str {
        match &self.target {
            Some(t) if t.is_builtin() => "tool",
            Some(_) => "skill",
            None => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub parsed: ParsedToolCall,
    pub result: ToolCallResult,
}
