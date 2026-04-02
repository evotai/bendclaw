use reqwest::Client;
use reqwest::Response;
use reqwest::StatusCode;

use crate::types::http;
use crate::types::ErrorCode;

pub struct StreamApiError {
    pub status: StatusCode,
    pub text: String,
}

pub struct StreamFallbackBody {
    pub body: String,
    pub data: serde_json::Value,
}

pub fn build_http_client() -> crate::types::Result<Client> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| ErrorCode::llm_request(format!("failed to build HTTP client: {e}")))
}

pub fn truncate_for_log(text: &str) -> String {
    crate::types::truncate_bytes_on_char_boundary(text, 512)
}

pub fn is_streaming_content_type(content_type: &str) -> bool {
    content_type.contains("stream") || content_type.contains("event-stream")
}

pub fn api_error_message(provider: &str, status: StatusCode, text: &str) -> String {
    format!("{provider} API error {status}: {text}")
}

/// Known phrases that indicate the prompt exceeded the model's context window.
/// Covers Anthropic, OpenAI, Google, Bedrock, xAI, Groq, OpenRouter, llama.cpp,
/// LM Studio, MiniMax, Kimi, GitHub Copilot, and generic patterns.
const OVERFLOW_PHRASES: &[&str] = &[
    "prompt is too long",
    "input is too long",
    "exceeds the context window",
    "exceeds the maximum",
    "maximum prompt length",
    "reduce the length of the messages",
    "maximum context length",
    "exceeds the limit of",
    "exceeds the available context size",
    "greater than the context length",
    "context window exceeds limit",
    "exceeded model token limit",
    "context length exceeded",
    "context_length_exceeded",
    "too many tokens",
    "token limit exceeded",
];

/// Returns `true` when the HTTP status + body indicate a context overflow error.
pub fn is_context_overflow(status: StatusCode, message: &str) -> bool {
    // HTTP 413 is "Payload Too Large" — always overflow
    if status.as_u16() == 413 {
        return true;
    }
    // HTTP 400 with empty body is often overflow from some providers
    if status.as_u16() == 400 && message.trim().is_empty() {
        return true;
    }
    is_context_overflow_message(message)
}

/// Returns `true` when the error message text matches a known overflow pattern.
pub fn is_context_overflow_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    OVERFLOW_PHRASES.iter().any(|phrase| lower.contains(phrase))
}

/// Classify an API error response into the appropriate `ErrorCode`.
/// Detects context overflow, rate limits, and server errors before falling
/// back to the generic `llm_request`.
pub fn classify_api_error(provider: &str, status: StatusCode, text: &str) -> ErrorCode {
    let msg = api_error_message(provider, status, text);
    if is_context_overflow(status, text) {
        return ErrorCode::llm_context_overflow(msg);
    }
    if status.as_u16() == 429 {
        return ErrorCode::llm_rate_limit(msg);
    }
    if status.is_server_error() {
        return ErrorCode::llm_server(msg);
    }
    ErrorCode::llm_request(msg)
}

pub async fn stream_done(
    writer: &crate::llm::stream::StreamWriter,
    reason: &str,
    provider: &str,
    model: Option<String>,
) {
    writer
        .done_with_provider(reason, Some(provider.to_string()), model)
        .await;
}

pub fn request_ctx_model(request_ctx: &http::HttpRequestContext) -> &str {
    request_ctx.model.as_deref().unwrap_or("")
}

pub fn log_stream_api_error(
    provider: &str,
    request_ctx: &http::HttpRequestContext,
    request_id: &str,
    status: StatusCode,
    response_bytes: usize,
) {
    crate::observability::log::slog!(error, "llm", "stream_api_error",
        provider,
        model = %request_ctx_model(request_ctx),
        error_origin = %http::ErrorOrigin::from_status_code(status.as_u16()),
        status = %status,
        request_id = %request_id,
        response_bytes,
    );
}

pub fn log_request(provider: &str, model: &str, msg_count: usize) {
    crate::observability::log::slog!(info, "llm", "request", provider, model, msg_count,);
}

pub fn log_api_error(
    provider: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
    status: StatusCode,
    request_id: &str,
    response: &str,
) {
    crate::observability::log::slog!(error, "llm", "api_error",
        provider,
        model,
        base_url = %base_url,
        api_key = %crate::llm::provider::mask_api_key(api_key),
        error_origin = %http::ErrorOrigin::from_status_code(status.as_u16()),
        status = %status,
        request_id = %request_id,
        response = %truncate_for_log(response),
    );
}

pub fn log_stream_failed(provider: &str, model: &str, error: &str) {
    crate::observability::log::slog!(warn, "llm", "stream_failed",
        provider,
        model = %model,
        error = %error,
    );
}

pub fn log_stream_fallback(
    provider: &str,
    model: &str,
    request_id: Option<&str>,
    content_type: &str,
) {
    match request_id {
        Some(request_id) => crate::observability::log::slog!(
            warn,
            "llm",
            "stream_fallback",
            provider,
            model = %model,
            request_id = %request_id,
            content_type,
        ),
        None => crate::observability::log::slog!(
            warn,
            "llm",
            "stream_fallback",
            provider,
            model = %model,
            content_type,
        ),
    }
}

pub fn log_stream_event_error(
    provider: &str,
    model: &str,
    request_id: &str,
    error: &str,
    payload: &str,
) {
    crate::observability::log::slog!(error, "llm", "stream_event_error",
        provider,
        model = %model,
        request_id = %request_id,
        error = %error,
        payload = %truncate_for_log(payload),
    );
}

pub fn log_stream_body_fallback(
    provider: &str,
    model: &str,
    request_id: &str,
    content_type: &str,
    body: &str,
) {
    crate::observability::log::slog!(
        warn,
        "llm",
        "stream_body_fallback",
        provider,
        model = %model,
        request_id = %request_id,
        content_type,
        body_bytes = body.len(),
        body_preview = %truncate_for_log(body),
    );
}

pub async fn read_stream_error(
    resp: Response,
    endpoint: &str,
    request_ctx: &http::HttpRequestContext,
) -> StreamApiError {
    let status = resp.status();
    let text = http::read_text(
        resp,
        http::HttpRequestContext::new("llm", "read_error_body")
            .with_endpoint(endpoint)
            .with_model(request_ctx.model.clone().unwrap_or_default())
            .with_url(request_ctx.url.clone()),
    )
    .await
    .unwrap_or_default();

    StreamApiError { status, text }
}

pub async fn read_stream_fallback_body(
    resp: Response,
    endpoint: &str,
    request_ctx: &http::HttpRequestContext,
) -> std::result::Result<StreamFallbackBody, String> {
    let body = http::read_text(
        resp,
        http::HttpRequestContext::new("llm", "read_body")
            .with_endpoint(endpoint)
            .with_model(request_ctx.model.clone().unwrap_or_default())
            .with_url(request_ctx.url.clone()),
    )
    .await
    .map_err(crate::llm::http_adapter::to_stream_error)?;
    let data = serde_json::from_str(&body)
        .map_err(|e| format!("non-streaming response parse failed: {e}"))?;

    Ok(StreamFallbackBody { body, data })
}
