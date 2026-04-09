//! OpenAI-compatible JSON fallback handling.
//!
//! When the upstream returns `application/json` instead of `text/event-stream`,
//! this module classifies the response as either an error or a complete
//! OpenAI Chat Completions response and converts it accordingly.

use tokio::sync::mpsc;
use tracing::debug;

use super::types::*;
use crate::provider::model::OpenAiCompat;
use crate::provider::stream_fallback::FallbackEmitter;
use crate::provider::stream_http;
use crate::provider::traits::ProviderError;
use crate::provider::traits::StreamConfig;
use crate::provider::traits::StreamEvent;
use crate::types::*;

/// Handle a JSON response from an OpenAI-compatible endpoint.
///
/// - Error-shaped JSON → [`ProviderError`]
/// - Success-shaped JSON → emits [`StreamEvent`]s and returns [`Message`]
pub(crate) async fn handle_json_response(
    response: reqwest::Response,
    tx: mpsc::UnboundedSender<StreamEvent>,
    config: &StreamConfig,
    compat: &OpenAiCompat,
) -> Result<Message, ProviderError> {
    let value = stream_http::read_json_body(response).await?;

    // Check for error-shaped JSON first
    if value.get("error").is_some() {
        debug!("OpenAI-compat JSON fallback: error response detected");
        return Err(stream_http::classify_json_error(&value));
    }

    debug!("OpenAI-compat JSON fallback: parsing as success completion");
    parse_success_response(value, tx, config, compat)
}

/// Parse a successful OpenAI Chat Completions JSON response into stream events.
fn parse_success_response(
    value: serde_json::Value,
    tx: mpsc::UnboundedSender<StreamEvent>,
    config: &StreamConfig,
    _compat: &OpenAiCompat,
) -> Result<Message, ProviderError> {
    let response: OpenAiResponse = serde_json::from_value(value)
        .map_err(|e| ProviderError::Api(format!("Failed to parse OpenAI response: {e}")))?;

    let mut emitter = FallbackEmitter::new(tx);

    // Process first choice
    if let Some(choice) = response.choices.first() {
        let msg = &choice.message;

        // Reasoning / thinking content
        let reasoning = msg
            .reasoning_content
            .as_deref()
            .or(msg.reasoning.as_deref());
        if let Some(thinking) = reasoning {
            emitter.emit_thinking(thinking, None);
        }

        // Text content
        if let Some(text) = &msg.content {
            emitter.emit_text(text);
        }

        // Tool calls
        if let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                let arguments =
                    crate::provider::json_repair::try_repair_json(&tc.function.arguments)
                        .unwrap_or(serde_json::Value::Object(Default::default()));
                emitter.emit_tool_call(&tc.id, &tc.function.name, arguments);
            }
        }

        // Stop reason
        let stop_reason = match choice.finish_reason.as_deref() {
            Some("stop") => StopReason::Stop,
            Some("length") => StopReason::Length,
            Some("tool_calls") => StopReason::ToolUse,
            _ => StopReason::Stop,
        };
        emitter.set_stop_reason(stop_reason);
    }

    // Usage
    if let Some(u) = &response.usage {
        let cache_read = u
            .prompt_tokens_details
            .as_ref()
            .map(|d| d.cached_tokens)
            .unwrap_or(0);
        let usage = Usage {
            input: u.prompt_tokens,
            output: u.completion_tokens,
            total_tokens: u.total_tokens,
            cache_read,
            ..Default::default()
        };
        emitter.set_usage(usage);
    }

    let provider = config
        .model_config
        .as_ref()
        .map(|mc| mc.provider.clone())
        .unwrap_or_else(|| "openai".into());

    Ok(emitter.finalize(&config.model, &provider))
}
