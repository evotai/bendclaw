use serde_json::Map;
use serde_json::Value;

use super::parsed_tool_call::ParsedToolCall;
use crate::kernel::OperationMeta;
use crate::observability::server_log;

pub(crate) fn log_tool_parse_failed(
    tool_name: &str,
    tool_call_id: &str,
    raw_arguments: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(
        warn,
        "tool",
        "parse_failed",
        tool_name = %tool_name,
        tool_call_id = %tool_call_id,
        raw_arguments = %raw_arguments,
        error = %error,
    );
}

pub(crate) fn log_tool_timed_out(tool: &str, tool_call_id: &str) {
    crate::observability::log::slog!(
        warn,
        "tool",
        "timed_out",
        tool = %tool,
        tool_call_id = %tool_call_id,
    );
}

pub(crate) fn log_tool_started(ctx: server_log::ServerCtx<'_>, parsed: &ParsedToolCall) {
    crate::observability::log::run_log!(info, ctx, "tool", "started",
        msg = format!("    tool [{}] started", parsed.call.name),
        tool_name = %parsed.call.name,
        tool_kind = %parsed.kind_str(),
        bytes = parsed.arguments.to_string().len() as u64,
        tool_call_id = %parsed.call.id,
    );
}

pub(crate) fn build_tool_started_payload(
    mut payload: Map<String, Value>,
    parsed: &ParsedToolCall,
) -> Map<String, Value> {
    payload.insert(
        "tool_call_id".to_string(),
        serde_json::json!(parsed.call.id.clone()),
    );
    payload.insert(
        "tool_name".to_string(),
        serde_json::json!(parsed.call.name.clone()),
    );
    payload.insert(
        "arguments".to_string(),
        serde_json::json!(parsed.arguments.clone()),
    );
    payload
}

pub(crate) fn log_tool_infra_error(name: &str, error: &str) {
    crate::observability::log::slog!(error, "tool", "infra_error",
        tool = %name,
        error = %error,
    );
}

pub(crate) fn log_tool_result(
    ctx: server_log::ServerCtx<'_>,
    parsed: &ParsedToolCall,
    meta: &OperationMeta,
    success: bool,
    error_text: Option<&str>,
    output_len: usize,
) {
    if success {
        crate::observability::log::run_log!(info, ctx, "tool", "completed",
            msg = format!("    tool [{}] completed {}ms", parsed.call.name, meta.duration_ms),
            tool_name = %parsed.call.name,
            tool_kind = %parsed.kind_str(),
            summary = %meta.summary,
            elapsed_ms = meta.duration_ms,
            bytes = output_len as u64,
            tool_call_id = %parsed.call.id,
        );
    } else {
        crate::observability::log::run_log!(error, ctx, "tool", "failed",
            msg = format!("    tool [{}] failed", parsed.call.name),
            tool_name = %parsed.call.name,
            tool_kind = %parsed.kind_str(),
            error = %error_text.unwrap_or(""),
            summary = %meta.summary,
            elapsed_ms = meta.duration_ms,
            bytes = output_len as u64,
            tool_call_id = %parsed.call.id,
        );
    }
}

pub(crate) fn build_tool_result_payload(
    mut payload: Map<String, Value>,
    parsed: &ParsedToolCall,
    success: bool,
    output: &str,
    error_text: Option<&str>,
    meta: &OperationMeta,
) -> Map<String, Value> {
    payload.insert(
        "tool_call_id".to_string(),
        serde_json::json!(parsed.call.id.clone()),
    );
    payload.insert(
        "tool_name".to_string(),
        serde_json::json!(parsed.call.name.clone()),
    );
    payload.insert("success".to_string(), serde_json::json!(success));
    payload.insert("output".to_string(), serde_json::json!(output));
    payload.insert("error".to_string(), serde_json::json!(error_text));
    payload.insert("operation".to_string(), serde_json::json!(meta.clone()));
    payload
}
