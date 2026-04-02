//! Tool lifecycle Message construction.
//!
//! Pure functions — no trace, no events, no audit.

use std::sync::Arc;

use super::parsed_tool_call::DispatchOutcome;
use super::parsed_tool_call::ParsedToolCall;
use super::tool_result::ToolCallResult;
use crate::kernel::tools::run_labels::RunLabels;
use crate::kernel::Message;
use crate::kernel::OperationMeta;

/// Operation event emitted when a tool starts.
pub fn tool_started_message(parsed: &ParsedToolCall) -> Message {
    Message::operation_event(
        parsed.kind_str(),
        &parsed.call.name,
        "started",
        serde_json::json!({"tool_call_id": parsed.call.id, "arguments": parsed.arguments}),
    )
}

/// Operation event emitted when a tool completes successfully.
pub fn tool_completed_message(parsed: &ParsedToolCall, meta: &OperationMeta) -> Message {
    Message::operation_event(
        parsed.kind_str(),
        &parsed.call.name,
        "completed",
        serde_json::json!({"tool_call_id": parsed.call.id, "duration_ms": meta.duration_ms}),
    )
}

/// Operation event emitted when a tool fails.
pub fn tool_failed_message(
    parsed: &ParsedToolCall,
    meta: &OperationMeta,
    error: String,
) -> Message {
    Message::operation_event(
        parsed.kind_str(),
        &parsed.call.name,
        "failed",
        serde_json::json!({"tool_call_id": parsed.call.id, "duration_ms": meta.duration_ms, "error": error}),
    )
}

/// Tool result message that goes back into the LLM transcript.
pub fn tool_result_message(outcome: &DispatchOutcome, labels: &Arc<RunLabels>) -> Message {
    let p = &outcome.parsed;
    let meta = outcome.result.operation().clone();
    let (output, success) = match &outcome.result {
        ToolCallResult::Success(out, _) => (out.clone(), true),
        ToolCallResult::ToolError(msg, _) | ToolCallResult::InfraError(msg, _) => {
            (format!("Error: {msg}"), false)
        }
    };
    Message::tool_result_with_operation(&p.call.id, &p.call.name, &output, success, meta)
        .with_run_id(labels.run_id.clone())
}
