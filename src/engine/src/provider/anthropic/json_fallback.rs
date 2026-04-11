//! Anthropic JSON fallback handling.
//!
//! When the upstream returns `application/json` instead of `text/event-stream`,
//! this module classifies the response as either an error or a complete
//! Anthropic Messages API response and converts it accordingly.

use tokio::sync::mpsc;
use tracing::debug;

use crate::provider::error::ProviderError;
use crate::provider::stream_fallback::FallbackEmitter;
use crate::provider::stream_http;
use crate::provider::traits::StreamConfig;
use crate::provider::traits::StreamEvent;
use crate::types::*;

/// Handle a JSON response from the Anthropic Messages API.
///
/// - Error-shaped JSON → [`ProviderError`]
/// - Success-shaped JSON → emits [`StreamEvent`]s and returns [`Message`]
pub(crate) async fn handle_json_response(
    response: reqwest::Response,
    tx: mpsc::UnboundedSender<StreamEvent>,
    config: &StreamConfig,
) -> Result<Message, ProviderError> {
    let value = stream_http::read_json_body(response).await?;

    // Check for error-shaped JSON first
    if is_error_response(&value) {
        debug!("Anthropic JSON fallback: error response detected");
        return Err(stream_http::classify_json_error(&value));
    }

    debug!("Anthropic JSON fallback: parsing as success completion");
    parse_success_response(value, tx, config)
}

/// Check if the JSON body looks like an Anthropic error response.
///
/// Anthropic errors have the shape:
/// ```json
/// { "type": "error", "error": { "type": "...", "message": "..." } }
/// ```
fn is_error_response(value: &serde_json::Value) -> bool {
    // Explicit error type
    if value.get("type").and_then(|t| t.as_str()) == Some("error") {
        return true;
    }
    // Has an error object
    if value.get("error").is_some() {
        return true;
    }
    false
}

/// Parse a successful Anthropic Messages API JSON response into stream events.
fn parse_success_response(
    value: serde_json::Value,
    tx: mpsc::UnboundedSender<StreamEvent>,
    config: &StreamConfig,
) -> Result<Message, ProviderError> {
    let mut emitter = FallbackEmitter::new(tx);

    // Parse content blocks
    if let Some(blocks) = value.get("content").and_then(|c| c.as_array()) {
        for block in blocks {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        emitter.emit_text(text);
                    }
                }
                "thinking" => {
                    let thinking = block.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                    let signature = block
                        .get("signature")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string());
                    emitter.emit_thinking(thinking, signature);
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = block.get("input").cloned().unwrap_or(serde_json::json!({}));
                    emitter.emit_tool_call(&id, &name, arguments);
                }
                _ => {
                    debug!("Anthropic JSON fallback: unknown content block type: {block_type}");
                }
            }
        }
    }

    // Parse usage
    if let Some(u) = value.get("usage") {
        let mut usage = Usage::default();
        if let Some(v) = u.get("input_tokens").and_then(|v| v.as_u64()) {
            usage.input = v;
        }
        if let Some(v) = u.get("output_tokens").and_then(|v| v.as_u64()) {
            usage.output = v;
        }
        if let Some(v) = u.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
            usage.cache_read = v;
        }
        if let Some(v) = u
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
        {
            usage.cache_write = v;
        }
        emitter.set_usage(usage);
    }

    // Parse stop reason
    let stop_reason = match value.get("stop_reason").and_then(|s| s.as_str()) {
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::Length,
        _ => StopReason::Stop,
    };
    emitter.set_stop_reason(stop_reason);

    Ok(emitter.finalize(&config.model, "anthropic"))
}
