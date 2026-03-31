use super::result::ToolCallResult;
use crate::llm::message::ToolCall;

#[derive(Debug, Clone, Copy)]
pub enum DispatchKind {
    Tool,
    Skill,
}

impl DispatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub call: ToolCall,
    pub arguments: serde_json::Value,
    pub kind: DispatchKind,
}

#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub parsed: ParsedToolCall,
    pub result: ToolCallResult,
}
