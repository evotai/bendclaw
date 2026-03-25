use serde_json::Map;
use serde_json::Value;

use super::engine::Engine;
use crate::kernel::run::dispatcher::ParsedToolCall;
use crate::kernel::run::run_loop::AbortSignal;
use crate::kernel::run::run_loop::LLMResponse;
use crate::kernel::run::run_loop::RunLoopState;
use crate::kernel::OperationMeta;
use crate::llm::message::ChatMessage;
use crate::llm::tool::ToolSchema;
use crate::observability::server_log;

pub(super) struct PreparedLlmRequest {
    pub(super) chat_messages: Vec<ChatMessage>,
    pub(super) active_tools: Vec<ToolSchema>,
    pub(super) request_payload: Map<String, Value>,
    pub(super) request_bytes: u64,
}

pub(super) struct PreparedLlmRequestSummary {
    pub(super) rows: u64,
    pub(super) tool_count: usize,
    pub(super) last_role: String,
    pub(super) last_user: String,
    pub(super) last_assistant: String,
    pub(super) chat_tail: String,
}

impl Engine {
    pub(super) fn prepare_llm_request(&self, iteration: u32) -> PreparedLlmRequest {
        let mut chat_messages = Vec::new();
        if !self.ctx.system_prompt.is_empty() {
            chat_messages.push(ChatMessage::system(&*self.ctx.system_prompt).with_cache_control());
        }
        chat_messages.extend(crate::kernel::run::fmt::to_chat_messages(
            &self.ctx.messages,
        ));

        let active_tools = self.ctx.tool_view.tool_schemas();
        let mut request_payload = self.audit_payload(iteration);
        request_payload.insert(
            "model".to_string(),
            serde_json::json!(self.ctx.model.to_string()),
        );
        request_payload.insert(
            "temperature".to_string(),
            serde_json::json!(self.ctx.temperature),
        );
        request_payload.insert(
            "tool_strategy".to_string(),
            serde_json::json!(format!("{:?}", self.ctx.tool_view.strategy())),
        );
        request_payload.insert(
            "messages".to_string(),
            serde_json::json!(chat_messages.clone()),
        );
        request_payload.insert("tools".to_string(), serde_json::json!(active_tools.clone()));
        let request_bytes = serde_json::to_vec(&Value::Object(request_payload.clone()))
            .map(|body| body.len() as u64)
            .unwrap_or(0);

        PreparedLlmRequest {
            chat_messages,
            active_tools,
            request_payload,
            request_bytes,
        }
    }
}

pub(super) fn summarize_prepared_llm_request(
    prepared: &PreparedLlmRequest,
) -> PreparedLlmRequestSummary {
    PreparedLlmRequestSummary {
        rows: prepared.chat_messages.len() as u64,
        tool_count: prepared.active_tools.len(),
        last_role: prepared
            .chat_messages
            .last()
            .map(|msg| msg.role.to_string())
            .unwrap_or_default(),
        last_user: last_chat_preview(&prepared.chat_messages, crate::base::Role::User),
        last_assistant: last_chat_preview(&prepared.chat_messages, crate::base::Role::Assistant),
        chat_tail: chat_tail_summary(&prepared.chat_messages, 6),
    }
}

pub(super) fn log_llm_context(ctx: server_log::ServerCtx<'_>, summary: &PreparedLlmRequestSummary) {
    crate::observability::log::run_log!(info, ctx, "context", "llm_prepared",
        msg = "llm context prepared",
        rows = summary.rows,
        tool_count = summary.tool_count,
        last_role = %summary.last_role,
        last_user = %summary.last_user,
        last_assistant = %summary.last_assistant,
        chat_tail = %summary.chat_tail,
    );
}

pub(super) fn log_llm_request(
    ctx: server_log::ServerCtx<'_>,
    model: &str,
    tool_strategy: &str,
    temperature: f64,
    request_bytes: u64,
    summary: &PreparedLlmRequestSummary,
) {
    crate::observability::log::run_log!(info, ctx, "llm", "request",
        msg = format!("    llm \u{2192} {model}"),
        model = %model,
        tool_strategy = %tool_strategy,
        tool_count = summary.tool_count,
        temperature,
        attempt = ctx.turn,
        rows = summary.rows,
        bytes = request_bytes,
    );
}

pub(super) fn build_llm_response_payload(
    mut payload: Map<String, Value>,
    model_fallback: &str,
    turn: &LLMResponse,
    ttft_ms: u64,
) -> Map<String, Value> {
    payload.insert(
        "model".to_string(),
        serde_json::json!(turn.model().unwrap_or(model_fallback)),
    );
    payload.insert("provider".to_string(), serde_json::json!(turn.provider()));
    payload.insert(
        "finish_reason".to_string(),
        serde_json::json!(turn.finish_reason()),
    );
    payload.insert("text".to_string(), serde_json::json!(turn.text()));
    payload.insert(
        "content_blocks".to_string(),
        serde_json::json!(turn.content_blocks()),
    );
    payload.insert(
        "tool_calls".to_string(),
        serde_json::json!(turn.tool_calls()),
    );
    payload.insert("usage".to_string(), serde_json::json!(turn.usage()));
    payload.insert("ttft_ms".to_string(), serde_json::json!(ttft_ms));
    payload.insert(
        "chunk_count".to_string(),
        serde_json::json!(turn.chunk_count() as u64),
    );
    payload.insert("bytes".to_string(), serde_json::json!(turn.bytes()));
    payload
}

pub(super) fn log_llm_failure(
    ctx: server_log::ServerCtx<'_>,
    model_fallback: &str,
    turn: &LLMResponse,
    error: &str,
    elapsed_ms: u64,
    ttft_ms: u64,
) {
    crate::observability::log::run_log!(error, ctx, "llm", "failed",
        msg = format!("    llm \u{2717} {} {elapsed_ms}ms", turn.finish_reason()),
        model = %turn.model().unwrap_or(model_fallback),
        provider = %turn.provider().unwrap_or(""),
        finish_reason = %turn.finish_reason(),
        error = %error,
        tool_calls = turn.tool_calls().len(),
        elapsed_ms,
        tokens = turn.usage().total_tokens,
        ttft_ms,
        attempt = ctx.turn,
        bytes = turn.bytes(),
        chunk_count = turn.chunk_count(),
    );
}

pub(super) fn log_llm_success(
    ctx: server_log::ServerCtx<'_>,
    model_fallback: &str,
    turn: &LLMResponse,
    elapsed_ms: u64,
    ttft_ms: u64,
) {
    crate::observability::log::run_log!(info, ctx, "llm", "completed",
        msg = format!("    llm \u{2190} {} {elapsed_ms}ms", turn.finish_reason()),
        model = %turn.model().unwrap_or(model_fallback),
        provider = %turn.provider().unwrap_or(""),
        finish_reason = %turn.finish_reason(),
        tool_calls = turn.tool_calls().len(),
        elapsed_ms,
        tokens = turn.usage().total_tokens,
        ttft_ms,
        attempt = ctx.turn,
        bytes = turn.bytes(),
        chunk_count = turn.chunk_count(),
    );
}

pub(super) fn log_llm_cancelled() {
    crate::observability::log::slog!(debug, "llm", "cancelled",);
}

pub(super) fn log_llm_collected(turn: &LLMResponse, chunk_count: u32, bytes: u64) {
    crate::observability::log::slog!(debug, "llm", "collected",
        tool_calls = turn.tool_calls().len(),
        prompt_tokens = turn.usage().prompt_tokens,
        completion_tokens = turn.usage().completion_tokens,
        finish_reason = %turn.finish_reason(),
        has_error = turn.has_error(),
        ttft_ms = turn.ttft_ms().unwrap_or(0),
        chunk_count,
        bytes,
    );
}

pub(super) fn log_turn_started(
    ctx: server_log::ServerCtx<'_>,
    iteration: u32,
    tool_strategy: &str,
    state: &RunLoopState,
    message_count: usize,
) {
    crate::observability::log::run_log!(info, ctx, "turn", "started",
        msg = format!("  iter-{iteration}"),
        tool_strategy = %tool_strategy,
        max_context_tokens = state.max_context_tokens(),
        message_count,
    );
}

pub(super) fn build_turn_completed_payload(
    mut payload: Map<String, Value>,
    status: &str,
    turn: &LLMResponse,
    extra: &[(&str, Value)],
) -> Map<String, Value> {
    payload.insert("status".to_string(), serde_json::json!(status));
    payload.insert(
        "finish_reason".to_string(),
        serde_json::json!(turn.finish_reason()),
    );
    payload.insert(
        "tool_calls".to_string(),
        serde_json::json!(turn.tool_calls().len() as u64),
    );
    for (k, v) in extra {
        payload.insert(k.to_string(), v.clone());
    }
    payload
}

pub(super) fn log_turn_completed(
    ctx: server_log::ServerCtx<'_>,
    iteration: u32,
    status: &str,
    turn: &LLMResponse,
) {
    match status {
        "failed" => crate::observability::log::run_log!(error, ctx, "turn", status,
            msg = format!("  iter-{iteration} {status}"),
            finish_reason = %turn.finish_reason(),
            tool_calls = turn.tool_calls().len(),
            tokens = turn.usage().total_tokens,
            bytes = turn.bytes(),
            chunk_count = turn.chunk_count(),
        ),
        "aborted" => crate::observability::log::run_log!(warn, ctx, "turn", status,
            msg = format!("  iter-{iteration} {status}"),
            finish_reason = %turn.finish_reason(),
            tool_calls = turn.tool_calls().len(),
            tokens = turn.usage().total_tokens,
            bytes = turn.bytes(),
            chunk_count = turn.chunk_count(),
        ),
        _ => crate::observability::log::run_log!(info, ctx, "turn", status,
            msg = format!("  iter-{iteration} {status}"),
            finish_reason = %turn.finish_reason(),
            tool_calls = turn.tool_calls().len(),
            tokens = turn.usage().total_tokens,
            bytes = turn.bytes(),
            chunk_count = turn.chunk_count(),
        ),
    }
}

pub(super) fn log_run_finished(
    elapsed_ms: u64,
    iterations: u32,
    prompt_tokens: u64,
    completion_tokens: u64,
    ttft_ms: u64,
    stop_reason: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(debug, "run", "finished",
        elapsed_ms,
        iterations,
        prompt_tokens,
        completion_tokens,
        ttft_ms,
        stop_reason = %stop_reason,
    );
}

pub(super) fn log_abort_signal(
    signal: AbortSignal,
    iterations: u32,
    max_iterations: u32,
    max_duration_secs: u64,
) {
    match signal {
        AbortSignal::MaxIterations => crate::observability::log::slog!(
            warn,
            "run",
            "aborted",
            reason = "max_iterations",
            iterations,
            max = max_iterations,
        ),
        AbortSignal::Timeout => crate::observability::log::slog!(
            warn,
            "run",
            "aborted",
            reason = "timeout",
            max_duration_secs,
        ),
        _ => {}
    }
}

pub(super) fn log_tool_started(ctx: server_log::ServerCtx<'_>, parsed: &ParsedToolCall) {
    crate::observability::log::run_log!(info, ctx, "tool", "started",
        msg = format!("    tool [{}] started", parsed.call.name),
        tool_name = %parsed.call.name,
        tool_kind = %parsed.kind.as_str(),
        bytes = parsed.arguments.to_string().len() as u64,
        tool_call_id = %parsed.call.id,
    );
}

pub(super) fn build_tool_started_payload(
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

pub(super) fn log_tool_infra_error(name: &str, error: &str) {
    crate::observability::log::slog!(error, "tool", "infra_error",
        tool = %name,
        error = %error,
    );
}

pub(super) fn log_message_injected(session_id: &str) {
    crate::observability::log::slog!(info, "run", "message_injected",
        session_id = %session_id,
    );
}

pub(super) fn log_tool_result(
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
            tool_kind = %parsed.kind.as_str(),
            summary = %meta.summary,
            elapsed_ms = meta.duration_ms,
            bytes = output_len as u64,
            tool_call_id = %parsed.call.id,
        );
    } else {
        crate::observability::log::run_log!(error, ctx, "tool", "failed",
            msg = format!("    tool [{}] failed", parsed.call.name),
            tool_name = %parsed.call.name,
            tool_kind = %parsed.kind.as_str(),
            error = %error_text.unwrap_or(""),
            summary = %meta.summary,
            elapsed_ms = meta.duration_ms,
            bytes = output_len as u64,
            tool_call_id = %parsed.call.id,
        );
    }
}

pub(super) fn build_tool_result_payload(
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

fn chat_tail_summary(messages: &[ChatMessage], limit: usize) -> String {
    let start = messages.len().saturating_sub(limit);
    messages[start..]
        .iter()
        .map(|msg| {
            let mut text = server_log::preview_text(&msg.text());
            if msg.role == crate::base::Role::Assistant && !msg.tool_calls.is_empty() {
                text = format!("{text} [tool_calls:{}]", msg.tool_calls.len());
            }
            format!("{}: {}", msg.role, text)
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn last_chat_preview(messages: &[ChatMessage], role: crate::base::Role) -> String {
    messages
        .iter()
        .rev()
        .find(|msg| msg.role == role)
        .map(|msg| server_log::preview_text(&msg.text()))
        .unwrap_or_default()
}
