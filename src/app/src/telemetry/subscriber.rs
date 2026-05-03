//! OTel span subscriber — converts AgentEvent stream into OTel spans.
//!
//! This module implements the span state machine that tracks open spans
//! and maps evot engine events to OTel Gen AI semantic conventions.

use std::collections::HashMap;
use std::time::SystemTime;

use opentelemetry::trace::Span;
use opentelemetry::trace::SpanKind;
use opentelemetry::trace::Status;
use opentelemetry::trace::TraceContextExt;
use opentelemetry::trace::Tracer;
use opentelemetry::trace::TracerProvider;
use opentelemetry::Array;
use opentelemetry::Context;
use opentelemetry::KeyValue;
use opentelemetry::StringValue;
use opentelemetry::Value;

use super::config::TelemetryConfig;

// ---------------------------------------------------------------------------
// Gen AI semantic convention attribute keys
// ---------------------------------------------------------------------------

const GEN_AI_OPERATION_NAME: &str = "gen_ai.operation.name";
const GEN_AI_PROVIDER_NAME: &str = "gen_ai.provider.name";
const GEN_AI_REQUEST_MODEL: &str = "gen_ai.request.model";
const GEN_AI_RESPONSE_MODEL: &str = "gen_ai.response.model";
const GEN_AI_REQUEST_STREAM: &str = "gen_ai.request.stream";
const GEN_AI_REQUEST_MAX_TOKENS: &str = "gen_ai.request.max_tokens";
const GEN_AI_REQUEST_TEMPERATURE: &str = "gen_ai.request.temperature";
const GEN_AI_USAGE_INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";
const GEN_AI_USAGE_OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
const GEN_AI_USAGE_CACHE_READ: &str = "gen_ai.usage.cache_read.input_tokens";
const GEN_AI_USAGE_CACHE_CREATION: &str = "gen_ai.usage.cache_creation.input_tokens";
const GEN_AI_RESPONSE_FINISH_REASONS: &str = "gen_ai.response.finish_reasons";
const GEN_AI_OUTPUT_MESSAGES: &str = "gen_ai.output.messages";
const GEN_AI_INPUT_MESSAGES: &str = "gen_ai.input.messages";
const GEN_AI_TOOL_DEFINITIONS: &str = "gen_ai.tool.definitions";
const GEN_AI_CONVERSATION_ID: &str = "gen_ai.conversation.id";
const GEN_AI_TOOL_NAME: &str = "gen_ai.tool.name";
const GEN_AI_TOOL_CALL_ID: &str = "gen_ai.tool.call.id";
const GEN_AI_TOOL_CALL_ARGS: &str = "gen_ai.tool.call.arguments";
const GEN_AI_TOOL_CALL_RESULT: &str = "gen_ai.tool.call.result";
const SERVER_ADDRESS: &str = "server.address";
const SERVER_PORT: &str = "server.port";
const ERROR_TYPE: &str = "error.type";

// Langfuse enhancement attributes
const LANGFUSE_COMPLETION_START_TIME: &str = "langfuse.observation.completion_start_time";

// ---------------------------------------------------------------------------
// SpanState — tracks open spans for a single agent run
// ---------------------------------------------------------------------------

struct LlmSpanState {
    cx: Context,
    start_time: SystemTime,
}

/// Manages OTel span lifecycle for a single agent run.
pub struct TelemetrySubscriber {
    config: TelemetryConfig,
    session_id: String,
    root_span: Option<Context>,
    /// Context with root span active — parent for LLM spans.
    root_cx: Option<Context>,
    /// Current LLM spans keyed by (turn, attempt).
    llm_spans: HashMap<(usize, usize), LlmSpanState>,
    /// Tool spans keyed by tool_call_id.
    tool_spans: HashMap<String, opentelemetry::global::BoxedSpan>,
    /// Context of the most recently started LLM span (for parenting tool spans).
    current_llm_cx: Option<Context>,
}

impl TelemetrySubscriber {
    /// Create a new subscriber for a run. Returns `None` if telemetry is disabled.
    pub fn new(config: &TelemetryConfig, session_id: &str) -> Option<Self> {
        if !config.is_enabled() {
            return None;
        }
        Some(Self {
            config: config.clone(),
            session_id: session_id.to_string(),
            root_span: None,
            root_cx: None,
            llm_spans: HashMap::new(),
            tool_spans: HashMap::new(),
            current_llm_cx: None,
        })
    }

    /// Called on AgentStart — creates the root invoke_agent span.
    pub fn on_agent_start(&mut self) {
        let tracer = opentelemetry::global::tracer_provider().tracer("evot");
        let span = tracer
            .span_builder("invoke_agent evot")
            .with_kind(SpanKind::Internal)
            .with_attributes(vec![
                KeyValue::new(GEN_AI_OPERATION_NAME, "invoke_agent"),
                KeyValue::new(GEN_AI_CONVERSATION_ID, self.session_id.clone()),
                KeyValue::new("session.id", self.session_id.clone()),
            ])
            .start(&tracer);

        let root_cx = Context::current_with_span(span);
        self.root_span = Some(root_cx.clone());
        self.root_cx = Some(root_cx);
    }

    /// Called on AgentEnd — ends the root span.
    pub fn on_agent_end(&mut self) {
        if let Some(cx) = self.root_span.take() {
            cx.span().end();
        }
        self.root_cx = None;
    }

    /// Called on LlmCallStart.
    #[allow(clippy::too_many_arguments)]
    pub fn on_llm_call_start(
        &mut self,
        turn: usize,
        attempt: usize,
        model: &str,
        provider_name: &str,
        server_address: Option<&str>,
        server_port: Option<u16>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        messages: &[evot_engine::Message],
        tools: &[evot_engine::provider::ToolDefinition],
    ) {
        let tracer = opentelemetry::global::tracer_provider().tracer("evot");
        let span_name = format!("chat {}", model);

        let mut attrs = vec![
            KeyValue::new(GEN_AI_OPERATION_NAME, "chat"),
            KeyValue::new(GEN_AI_PROVIDER_NAME, provider_name.to_string()),
            KeyValue::new(GEN_AI_REQUEST_MODEL, model.to_string()),
            KeyValue::new(GEN_AI_REQUEST_STREAM, true),
            KeyValue::new(GEN_AI_CONVERSATION_ID, self.session_id.clone()),
        ];

        if let Some(addr) = server_address {
            attrs.push(KeyValue::new(SERVER_ADDRESS, addr.to_string()));
        }
        if let Some(port) = server_port {
            attrs.push(KeyValue::new(SERVER_PORT, port as i64));
        }
        if let Some(mt) = max_tokens {
            attrs.push(KeyValue::new(GEN_AI_REQUEST_MAX_TOKENS, mt as i64));
        }
        if let Some(temp) = temperature {
            attrs.push(KeyValue::new(GEN_AI_REQUEST_TEMPERATURE, temp as f64));
        }
        if self.config.capture_content {
            if let Ok(json) = build_input_messages(messages) {
                attrs.push(KeyValue::new(GEN_AI_INPUT_MESSAGES, json));
            }
            if !tools.is_empty() {
                if let Ok(json) = serde_json::to_string(tools) {
                    attrs.push(KeyValue::new(GEN_AI_TOOL_DEFINITIONS, json));
                }
            }
        }

        // Parent: root span context
        let parent_cx = self
            .root_cx
            .as_ref()
            .cloned()
            .unwrap_or_else(Context::current);

        let span = tracer
            .span_builder(span_name.clone())
            .with_kind(SpanKind::Client)
            .with_attributes(attrs)
            .start_with_context(&tracer, &parent_cx);

        let now = SystemTime::now();
        let llm_cx = parent_cx.with_span(span);
        self.current_llm_cx = Some(llm_cx.clone());

        self.llm_spans.insert((turn, attempt), LlmSpanState {
            cx: llm_cx,
            start_time: now,
        });
    }

    /// Called on LlmCallEnd.
    #[allow(clippy::too_many_arguments)]
    pub fn on_llm_call_end(
        &mut self,
        turn: usize,
        attempt: usize,
        response_model: Option<&str>,
        usage_input: u64,
        usage_output: u64,
        cache_read: u64,
        cache_write: u64,
        finish_reason: Option<&str>,
        error: Option<&str>,
        ttft_ms: u64,
        stop_reason: &evot_engine::StopReason,
        content: &[evot_engine::Content],
    ) {
        if let Some(state) = self.llm_spans.remove(&(turn, attempt)) {
            let span = state.cx.span();
            if let Some(rm) = response_model {
                span.set_attribute(KeyValue::new(GEN_AI_RESPONSE_MODEL, rm.to_string()));
            }
            span.set_attribute(KeyValue::new(GEN_AI_USAGE_INPUT_TOKENS, usage_input as i64));
            span.set_attribute(KeyValue::new(
                GEN_AI_USAGE_OUTPUT_TOKENS,
                usage_output as i64,
            ));

            if cache_read > 0 {
                span.set_attribute(KeyValue::new(GEN_AI_USAGE_CACHE_READ, cache_read as i64));
            }
            if cache_write > 0 {
                span.set_attribute(KeyValue::new(
                    GEN_AI_USAGE_CACHE_CREATION,
                    cache_write as i64,
                ));
            }

            if let Some(fr) = finish_reason {
                span.set_attribute(KeyValue::new(
                    GEN_AI_RESPONSE_FINISH_REASONS,
                    Value::Array(Array::String(vec![StringValue::from(fr.to_string())])),
                ));
            }

            // Langfuse: completion_start_time
            if ttft_ms > 0 {
                let completion_start = state.start_time + std::time::Duration::from_millis(ttft_ms);
                let iso = chrono::DateTime::<chrono::Utc>::from(completion_start)
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                span.set_attribute(KeyValue::new(LANGFUSE_COMPLETION_START_TIME, iso));
            }

            if let Some(err) = error {
                let error_type = classify_error(err);
                span.set_attribute(KeyValue::new(ERROR_TYPE, error_type));
                span.set_status(Status::error(err.to_string()));
            }

            // gen_ai.output.messages (Opt-In): serialize response as JSON string
            if self.config.capture_content && !content.is_empty() {
                if let Ok(json) = build_output_messages(content, stop_reason) {
                    span.set_attribute(KeyValue::new(GEN_AI_OUTPUT_MESSAGES, json));
                }
            }

            span.end();
        }
    }

    /// Called on ToolExecutionStart.
    pub fn on_tool_start(&mut self, tool_call_id: &str, tool_name: &str, args: &serde_json::Value) {
        let tracer = opentelemetry::global::tracer_provider().tracer("evot");
        let span_name = format!("execute_tool {}", tool_name);

        let mut attrs = vec![
            KeyValue::new(GEN_AI_OPERATION_NAME, "execute_tool"),
            KeyValue::new(GEN_AI_TOOL_NAME, tool_name.to_string()),
            KeyValue::new(GEN_AI_TOOL_CALL_ID, tool_call_id.to_string()),
            KeyValue::new(GEN_AI_CONVERSATION_ID, self.session_id.clone()),
        ];
        if self.config.capture_content {
            if let Ok(args) = serde_json::to_string(args) {
                attrs.push(KeyValue::new(GEN_AI_TOOL_CALL_ARGS, args));
            }
        }

        // Parent: current LLM span context (tools are children of the LLM call)
        let parent_cx = self
            .current_llm_cx
            .as_ref()
            .or(self.root_cx.as_ref())
            .cloned()
            .unwrap_or_else(Context::current);

        let span = tracer
            .span_builder(span_name)
            .with_kind(SpanKind::Internal)
            .with_attributes(attrs)
            .start_with_context(&tracer, &parent_cx);

        self.tool_spans.insert(tool_call_id.to_string(), span);
    }

    /// Called on ToolExecutionEnd.
    pub fn on_tool_end(
        &mut self,
        tool_call_id: &str,
        is_error: bool,
        _duration_ms: u64,
        result: Option<&serde_json::Value>,
    ) {
        if let Some(mut span) = self.tool_spans.remove(tool_call_id) {
            if self.config.capture_content {
                if let Some(result) = result {
                    if let Ok(output) = serde_json::to_string(result) {
                        span.set_attribute(KeyValue::new(GEN_AI_TOOL_CALL_RESULT, output));
                    }
                }
            }
            if is_error {
                span.set_attribute(KeyValue::new(ERROR_TYPE, "tool_error"));
                span.set_status(Status::error("tool execution failed"));
            }
            span.end();
        }
    }
}

impl Drop for TelemetrySubscriber {
    fn drop(&mut self) {
        // End any orphaned spans
        for (_, state) in self.llm_spans.drain() {
            let span = state.cx.span();
            span.set_status(Status::error("abandoned"));
            span.end();
        }
        for (_, mut span) in self.tool_spans.drain() {
            span.set_status(Status::error("abandoned"));
            span.end();
        }
        if let Some(cx) = self.root_span.take() {
            cx.span().set_status(Status::error("abandoned"));
            cx.span().end();
        }
        self.root_cx = None;
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Classify error strings into stable OTel error.type categories.
fn classify_error(err: &str) -> &'static str {
    let lower = err.to_lowercase();
    if lower.contains("rate limit") || lower.contains("429") {
        "rate_limit"
    } else if lower.contains("auth") || lower.contains("401") || lower.contains("403") {
        "authentication"
    } else if lower.contains("timeout") || lower.contains("timed out") {
        "timeout"
    } else if lower.contains("context") && lower.contains("overflow") {
        "context_overflow"
    } else if lower.contains("overloaded") || lower.contains("529") || lower.contains("503") {
        "server_overloaded"
    } else {
        "provider_error"
    }
}

/// Build `gen_ai.input.messages` JSON string per OTel Gen AI semantic conventions.
fn build_input_messages(messages: &[evot_engine::Message]) -> Result<String, serde_json::Error> {
    let messages: Vec<_> = messages
        .iter()
        .map(|message| match message {
            evot_engine::Message::User { content, .. } => serde_json::json!({
                "role": "user",
                "parts": content_parts(content),
            }),
            evot_engine::Message::Assistant {
                content,
                stop_reason,
                ..
            } => serde_json::json!({
                "role": "assistant",
                "parts": content_parts(content),
                "finish_reason": finish_reason_name(stop_reason),
            }),
            evot_engine::Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                ..
            } => serde_json::json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "name": tool_name,
                "is_error": is_error,
                "parts": content_parts(content),
            }),
        })
        .collect();

    serde_json::to_string(&messages)
}

fn content_parts(content: &[evot_engine::Content]) -> Vec<serde_json::Value> {
    let mut parts = Vec::new();
    for block in content {
        match block {
            evot_engine::Content::Text { text } => {
                parts.push(serde_json::json!({
                    "type": "text",
                    "content": text,
                }));
            }
            evot_engine::Content::ToolCall {
                id,
                name,
                arguments,
            } => {
                parts.push(serde_json::json!({
                    "type": "tool_call",
                    "id": id,
                    "name": name,
                    "arguments": arguments,
                }));
            }
            evot_engine::Content::Thinking { thinking, .. } => {
                parts.push(serde_json::json!({
                    "type": "text",
                    "content": thinking,
                }));
            }
            evot_engine::Content::Image { mime_type, .. } => {
                parts.push(serde_json::json!({
                    "type": "image",
                    "mime_type": mime_type,
                }));
            }
        }
    }
    parts
}

fn finish_reason_name(stop_reason: &evot_engine::StopReason) -> &'static str {
    match stop_reason {
        evot_engine::StopReason::Stop => "stop",
        evot_engine::StopReason::ToolUse => "tool_calls",
        evot_engine::StopReason::Length => "length",
        evot_engine::StopReason::Error => "error",
        evot_engine::StopReason::Aborted => "error",
    }
}

/// Build `gen_ai.output.messages` JSON string per OTel Gen AI semantic conventions.
///
/// Format: `[{"role": "assistant", "parts": [...], "finish_reason": "..."}]`
/// where parts contain `{"type": "text", "content": "..."}` and/or
/// `{"type": "tool_call", "id": "...", "name": "...", "arguments": {...}}`.
fn build_output_messages(
    content: &[evot_engine::Content],
    stop_reason: &evot_engine::StopReason,
) -> Result<String, serde_json::Error> {
    let mut parts = Vec::new();
    for block in content {
        match block {
            evot_engine::Content::Text { text } => {
                parts.push(serde_json::json!({
                    "type": "text",
                    "content": text,
                }));
            }
            evot_engine::Content::ToolCall {
                id,
                name,
                arguments,
            } => {
                parts.push(serde_json::json!({
                    "type": "tool_call",
                    "id": id,
                    "name": name,
                    "arguments": arguments,
                }));
            }
            evot_engine::Content::Thinking { thinking, .. } => {
                parts.push(serde_json::json!({
                    "type": "text",
                    "content": thinking,
                }));
            }
            _ => {}
        }
    }

    let finish_reason = finish_reason_name(stop_reason);

    let message = serde_json::json!([{
        "role": "assistant",
        "parts": parts,
        "finish_reason": finish_reason,
    }]);

    serde_json::to_string(&message)
}
