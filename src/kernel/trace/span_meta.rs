use serde::Serialize;

use crate::kernel::tools::operation::Impact;

/// Typed metadata for trace spans — eliminates hand-written JSON in engine.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SpanMeta {
    LlmTurn {
        iteration: u32,
    },
    LlmResult {
        finish_reason: String,
    },
    LlmCompleted {
        finish_reason: String,
        prompt_tokens: u64,
        completion_tokens: u64,
    },
    LlmFailed {
        finish_reason: String,
        error: String,
    },
    ToolStarted {
        tool_call_id: String,
        arguments: serde_json::Value,
    },
    ToolCompleted {
        tool_call_id: String,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        impact: Option<Impact>,
        summary: String,
    },
    ToolFailed {
        tool_call_id: String,
        duration_ms: u64,
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        impact: Option<Impact>,
        summary: String,
    },
    Empty {},
}

impl SpanMeta {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}
