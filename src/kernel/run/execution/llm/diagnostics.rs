use serde_json::Map;
use serde_json::Value;

use super::abort::AbortSignal;
use super::engine_state::RunLoopState;
use super::response_mapper::LLMResponse;
use super::turn_engine::Engine;
use crate::llm::message::ChatMessage;
use crate::llm::tool::ToolSchema;
use crate::observability::server_log;

fn tool_names(turn: &LLMResponse) -> String {
    turn.tool_calls()
        .iter()
        .map(|call| call.name.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) struct PreparedLlmRequest {
    pub(super) chat_messages: Vec<ChatMessage>,
    pub(super) active_tools: Vec<ToolSchema>,
    pub(super) request_payload: Map<String, Value>,
    pub(super) request_bytes: u64,
}

pub(super) struct PreparedLlmRequestSummary {
    pub(super) rows: u64,
    pub(super) tool_count: usize,
    pub(super) input_bytes: u64,
    pub(super) last_role: String,
    pub(super) last_user: String,
    pub(super) last_assistant: String,
    pub(super) role_counts: String,
    pub(super) tool_result_messages: usize,
    pub(super) assistant_tool_call_messages: usize,
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
        input_bytes: prepared.request_bytes,
        last_role: prepared
            .chat_messages
            .last()
            .map(|msg| msg.role.to_string())
            .unwrap_or_default(),
        last_user: last_chat_preview(&prepared.chat_messages, crate::base::Role::User),
        last_assistant: last_chat_preview(&prepared.chat_messages, crate::base::Role::Assistant),
        role_counts: chat_role_count_summary(&prepared.chat_messages),
        tool_result_messages: prepared
            .chat_messages
            .iter()
            .filter(|msg| msg.role == crate::base::Role::Tool)
            .count(),
        assistant_tool_call_messages: prepared
            .chat_messages
            .iter()
            .filter(|msg| msg.role == crate::base::Role::Assistant && !msg.tool_calls.is_empty())
            .count(),
    }
}

pub(super) fn log_llm_context_with_call_id(
    ctx: server_log::ServerCtx<'_>,
    llm_call_id: &str,
    summary: &PreparedLlmRequestSummary,
) {
    crate::observability::log::run_log!(info, ctx, "context", "llm_prepared",
        msg = "llm context prepared",
        llm_call_id = %llm_call_id,
        rows = summary.rows,
        tool_count = summary.tool_count,
        last_role = %summary.last_role,
        last_user = %summary.last_user,
        last_assistant = %summary.last_assistant,
        role_counts = %summary.role_counts,
        tool_result_messages = summary.tool_result_messages,
        assistant_tool_call_messages = summary.assistant_tool_call_messages,
        input_bytes = summary.input_bytes,
    );
}

pub(super) fn log_llm_request(
    ctx: server_log::ServerCtx<'_>,
    llm_call_id: &str,
    model: &str,
    tool_strategy: &str,
    temperature: f64,
    request_bytes: u64,
    summary: &PreparedLlmRequestSummary,
) {
    crate::observability::log::run_log!(info, ctx, "llm", "request",
        msg = format!("    llm \u{2192} {model}"),
        llm_call_id = %llm_call_id,
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
    llm_call_id: &str,
    model_fallback: &str,
    turn: &LLMResponse,
    ttft_ms: u64,
) -> Map<String, Value> {
    payload.insert("llm_call_id".to_string(), serde_json::json!(llm_call_id));
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
    llm_call_id: &str,
    model_fallback: &str,
    turn: &LLMResponse,
    error: &str,
    elapsed_ms: u64,
    ttft_ms: u64,
) {
    crate::observability::log::run_log!(error, ctx, "llm", "failed",
        msg = format!("    llm \u{2717} {} {elapsed_ms}ms", turn.finish_reason()),
        llm_call_id = %llm_call_id,
        model = %turn.model().unwrap_or(model_fallback),
        provider = %turn.provider().unwrap_or(""),
        finish_reason = %turn.finish_reason(),
        error = %error,
        tool_calls = turn.tool_calls().len(),
        tool_names = %tool_names(turn),
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
    llm_call_id: &str,
    model_fallback: &str,
    turn: &LLMResponse,
    elapsed_ms: u64,
    ttft_ms: u64,
) {
    crate::observability::log::run_log!(info, ctx, "llm", "completed",
        msg = format!("    llm \u{2190} {} {elapsed_ms}ms", turn.finish_reason()),
        llm_call_id = %llm_call_id,
        model = %turn.model().unwrap_or(model_fallback),
        provider = %turn.provider().unwrap_or(""),
        finish_reason = %turn.finish_reason(),
        tool_calls = turn.tool_calls().len(),
        tool_names = %tool_names(turn),
        elapsed_ms,
        tokens = turn.usage().total_tokens,
        ttft_ms,
        attempt = ctx.turn,
        bytes = turn.bytes(),
        chunk_count = turn.chunk_count(),
    );
}

pub(super) fn log_llm_final_output(
    ctx: server_log::ServerCtx<'_>,
    llm_call_id: &str,
    turn: &LLMResponse,
) {
    let tool_names_str = tool_names(turn);
    let content_blocks = turn.content_blocks();
    let thinking_preview = content_blocks
        .iter()
        .find_map(|block| match block {
            crate::kernel::run::result::ContentBlock::Thinking { thinking } => {
                Some(server_log::preview_text(thinking))
            }
            _ => None,
        })
        .unwrap_or_default();

    crate::observability::log::run_log!(info, ctx, "llm", "final_output",
        msg = "llm final output prepared",
        llm_call_id = %llm_call_id,
        finish_reason = %turn.finish_reason(),
        tool_calls = turn.tool_calls().len(),
        tool_names = %tool_names_str,
        text_preview = %server_log::preview_text(turn.text()),
        text_bytes = turn.text().len() as u64,
        thinking_preview = %thinking_preview,
        stream_event_summary = %turn.stream_event_summary(),
        content_blocks = content_blocks.len(),
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
    let names = tool_names(turn);
    if !names.is_empty() {
        payload.insert("tool_names".to_string(), serde_json::json!(names));
    }
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
    let names = tool_names(turn);
    match status {
        "failed" => crate::observability::log::run_log!(error, ctx, "turn", status,
            msg = format!("  iter-{iteration} {status}"),
            finish_reason = %turn.finish_reason(),
            tool_calls = turn.tool_calls().len(),
            tool_names = %names,
            tokens = turn.usage().total_tokens,
            bytes = turn.bytes(),
            chunk_count = turn.chunk_count(),
        ),
        "aborted" => crate::observability::log::run_log!(warn, ctx, "turn", status,
            msg = format!("  iter-{iteration} {status}"),
            finish_reason = %turn.finish_reason(),
            tool_calls = turn.tool_calls().len(),
            tool_names = %names,
            tokens = turn.usage().total_tokens,
            bytes = turn.bytes(),
            chunk_count = turn.chunk_count(),
        ),
        _ => crate::observability::log::run_log!(info, ctx, "turn", status,
            msg = format!("  iter-{iteration} {status}"),
            finish_reason = %turn.finish_reason(),
            tool_calls = turn.tool_calls().len(),
            tool_names = %names,
            tokens = turn.usage().total_tokens,
            bytes = turn.bytes(),
            chunk_count = turn.chunk_count(),
        ),
    }
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

pub(super) fn log_message_injected(session_id: &str) {
    crate::observability::log::slog!(info, "run", "message_injected",
        session_id = %session_id,
    );
}

fn chat_role_count_summary(messages: &[ChatMessage]) -> String {
    let system = messages
        .iter()
        .filter(|msg| msg.role == crate::base::Role::System)
        .count();
    let user = messages
        .iter()
        .filter(|msg| msg.role == crate::base::Role::User)
        .count();
    let assistant = messages
        .iter()
        .filter(|msg| msg.role == crate::base::Role::Assistant)
        .count();
    let tool = messages
        .iter()
        .filter(|msg| msg.role == crate::base::Role::Tool)
        .count();
    format!("system:{system},user:{user},assistant:{assistant},tool:{tool}")
}

fn last_chat_preview(messages: &[ChatMessage], role: crate::base::Role) -> String {
    messages
        .iter()
        .rev()
        .find(|msg| msg.role == role)
        .map(|msg| server_log::preview_text(&msg.text()))
        .unwrap_or_default()
}
