//! Shared HTTP transport helpers for stream providers.
//!
//! Provides response classification, body reading, JSON error extraction,
//! and SSE-from-response driving — reusable across all providers that
//! need to handle non-SSE JSON fallback.

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::sse::SseEvent;
use super::traits::is_context_overflow_message;
use super::traits::ProviderError;

// ---------------------------------------------------------------------------
// Response classification
// ---------------------------------------------------------------------------

/// How the upstream responded to a stream request.
#[derive(Debug, PartialEq, Eq)]
pub enum StreamResponseKind {
    /// Server returned an SSE-compatible content type.
    Streaming,
    /// Server returned `application/json` (could be success or error).
    Json,
    /// Unrecognised content type.
    Other(String),
}

/// Inspect the `content-type` header and classify the response.
pub fn classify_response(response: &reqwest::Response) -> StreamResponseKind {
    let ct = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if ct.contains("event-stream") || ct.contains("stream") {
        StreamResponseKind::Streaming
    } else if ct.contains("application/json") || ct.contains("json") {
        StreamResponseKind::Json
    } else {
        StreamResponseKind::Other(ct.to_string())
    }
}

// ---------------------------------------------------------------------------
// Request / status helpers
// ---------------------------------------------------------------------------

/// Send a stream request, mapping transport errors to [`ProviderError::Network`].
pub async fn send_stream_request(
    builder: reqwest::RequestBuilder,
) -> Result<reqwest::Response, ProviderError> {
    builder
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))
}

/// Check the HTTP status code. Non-2xx responses are read and classified.
pub async fn check_error_status(
    response: reqwest::Response,
) -> Result<reqwest::Response, ProviderError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    Err(ProviderError::classify(
        status,
        &format!("HTTP {status}: {body}"),
    ))
}

// ---------------------------------------------------------------------------
// Body reading
// ---------------------------------------------------------------------------

/// Read the full response body as text.
pub async fn read_text_body(response: reqwest::Response) -> Result<String, ProviderError> {
    response
        .text()
        .await
        .map_err(|e| ProviderError::Network(format!("Failed to read response body: {e}")))
}

/// Read the full response body and parse it as JSON.
pub async fn read_json_body(
    response: reqwest::Response,
) -> Result<serde_json::Value, ProviderError> {
    let text = read_text_body(response).await?;
    serde_json::from_str(&text)
        .map_err(|e| ProviderError::Api(format!("Failed to parse JSON response: {e}")))
}

// ---------------------------------------------------------------------------
// JSON error extraction & classification
// ---------------------------------------------------------------------------

/// Extract a human-readable error message from a JSON error body.
///
/// Tries common patterns across providers:
/// - `{ "error": { "type": "...", "message": "..." } }` (Anthropic)
/// - `{ "error": { "message": "..." } }` (OpenAI)
/// - `{ "message": "..." }` (generic)
/// - `{ "type": "..." }` (generic)
pub fn extract_json_error_message(value: &serde_json::Value) -> Option<String> {
    let error_obj = value.get("error");

    let error_type = error_obj
        .and_then(|e| e.get("type"))
        .and_then(|v| v.as_str())
        .or_else(|| value.get("type").and_then(|v| v.as_str()));

    let error_message = error_obj
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .or_else(|| value.get("message").and_then(|v| v.as_str()));

    match (error_type, error_message) {
        (Some(t), Some(m)) => Some(format!("{t}: {m}")),
        (None, Some(m)) => Some(m.to_string()),
        (Some(t), None) => Some(t.to_string()),
        (None, None) => None,
    }
}

/// Classify a JSON error body into a [`ProviderError`].
///
/// - Context overflow messages → [`ProviderError::ContextOverflow`]
/// - Everything else → [`ProviderError::Api`] (retryable)
pub fn classify_json_error(value: &serde_json::Value) -> ProviderError {
    let message = extract_json_error_message(value).unwrap_or_else(|| value.to_string());

    if is_context_overflow_message(&message) {
        ProviderError::ContextOverflow { message }
    } else {
        ProviderError::Api(message)
    }
}

// ---------------------------------------------------------------------------
// SSE from reqwest::Response
// ---------------------------------------------------------------------------

/// Drive SSE parsing from a raw `reqwest::Response` byte stream.
///
/// Parses standard SSE frames (`event:`, `data:`) and sends them through
/// the channel as [`SseEvent`]s. Returns when the stream ends, errors,
/// or is cancelled.
pub async fn drive_sse_response(
    response: reqwest::Response,
    tx: mpsc::UnboundedSender<SseEvent>,
    cancel: CancellationToken,
) -> Result<(), String> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                return Err("cancelled".into());
            }
            chunk = stream.next() => {
                match chunk {
                    None => {
                        // Stream ended — flush any remaining buffered event
                        flush_sse_buffer(&mut buffer, &tx);
                        return Ok(());
                    }
                    Some(Err(e)) => {
                        return Err(format!("Stream read error: {e}"));
                    }
                    Some(Ok(bytes)) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        // Process complete SSE frames
                        while let Some(pos) = buffer.find("\n\n") {
                            let frame = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();
                            if let Some(event) = parse_sse_frame(&frame) {
                                if tx.send(event).is_err() {
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Parse a single SSE frame into an [`SseEvent`].
fn parse_sse_frame(frame: &str) -> Option<SseEvent> {
    let mut event_type = String::new();
    let mut data_lines: Vec<&str> = Vec::new();

    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_type = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start_matches(' '));
        } else if line.starts_with(':') {
            // Comment line, skip
        }
    }

    if data_lines.is_empty() {
        return None;
    }

    let data = data_lines.join("\n");
    if event_type.is_empty() {
        event_type = "message".to_string();
    }

    debug!("SSE frame: event={event_type} data_len={}", data.len());

    Some(SseEvent {
        event: event_type,
        data,
    })
}

/// Flush any remaining partial SSE data in the buffer.
fn flush_sse_buffer(buffer: &mut String, tx: &mpsc::UnboundedSender<SseEvent>) {
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(event) = parse_sse_frame(trimmed) {
        let _ = tx.send(event);
    }
    buffer.clear();
}
