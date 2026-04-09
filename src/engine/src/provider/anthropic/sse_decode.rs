//! Anthropic SSE stream decoding.
//!
//! Parses Anthropic Messages API SSE events and translates them into
//! internal [`StreamEvent`]s while accumulating the final [`Message`].

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::types::*;
use crate::provider::sse::SseEvent;
use crate::provider::stream_http;
use crate::provider::traits::classify_sse_error_event;
use crate::provider::traits::ProviderError;
use crate::provider::traits::StreamConfig;
use crate::provider::traits::StreamEvent;
use crate::types::*;

/// Drive an Anthropic SSE stream from a raw HTTP response.
///
/// Parses SSE frames, translates Anthropic event types into [`StreamEvent`]s,
/// and returns the final assembled [`Message::Assistant`].
pub(crate) async fn decode_sse_stream(
    response: reqwest::Response,
    tx: mpsc::UnboundedSender<StreamEvent>,
    cancel: CancellationToken,
    config: &StreamConfig,
) -> Result<Message, ProviderError> {
    let (sse_tx, mut sse_rx) = mpsc::unbounded_channel::<SseEvent>();

    // Spawn SSE frame parser
    let sse_cancel = cancel.clone();
    let sse_handle =
        tokio::spawn(
            async move { stream_http::drive_sse_response(response, sse_tx, sse_cancel).await },
        );

    let mut content: Vec<Content> = Vec::new();
    let mut usage = Usage::default();
    let mut stop_reason = StopReason::Stop;

    let _ = tx.send(StreamEvent::Start);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                return Err(ProviderError::Cancelled);
            }
            event = sse_rx.recv() => {
                match event {
                    None => break,
                    Some(sse) => {
                        if let Some(err) = process_sse_event(
                            &sse,
                            &tx,
                            &mut content,
                            &mut usage,
                            &mut stop_reason,
                        )? {
                            // message_stop received
                            if err {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    // Wait for SSE driver to finish
    if let Ok(Err(e)) = sse_handle.await {
        // Only report if we haven't already built a message
        if content.is_empty() {
            return Err(ProviderError::Network(e));
        }
        debug!("SSE driver ended with error after content received: {e}");
    }

    let has_tool_calls = content
        .iter()
        .any(|c| matches!(c, Content::ToolCall { .. }));
    if has_tool_calls {
        stop_reason = StopReason::ToolUse;
    }

    let message = Message::Assistant {
        content,
        stop_reason,
        model: config.model.clone(),
        provider: "anthropic".into(),
        usage,
        timestamp: now_ms(),
        error_message: None,
    };

    let _ = tx.send(StreamEvent::Done {
        message: message.clone(),
    });
    Ok(message)
}

/// Process a single SSE event. Returns:
/// - `Ok(None)` — event processed, continue
/// - `Ok(Some(true))` — message_stop, break
/// - `Err(...)` — error event
fn process_sse_event(
    sse: &SseEvent,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    content: &mut Vec<Content>,
    usage: &mut Usage,
    stop_reason: &mut StopReason,
) -> Result<Option<bool>, ProviderError> {
    match sse.event.as_str() {
        "message_start" => {
            if let Ok(data) = serde_json::from_str::<AnthropicMessageStart>(&sse.data) {
                usage.input = data.message.usage.input_tokens;
                usage.cache_read = data.message.usage.cache_read_input_tokens;
                usage.cache_write = data.message.usage.cache_creation_input_tokens;
            }
        }
        "content_block_start" => {
            if let Ok(data) = serde_json::from_str::<AnthropicContentBlockStart>(&sse.data) {
                let idx = data.index as usize;
                match data.content_block {
                    AnthropicContentBlock::Text { .. } => {
                        while content.len() <= idx {
                            content.push(Content::Text {
                                text: String::new(),
                            });
                        }
                    }
                    AnthropicContentBlock::Thinking { .. } => {
                        while content.len() <= idx {
                            content.push(Content::Thinking {
                                thinking: String::new(),
                                signature: None,
                            });
                        }
                    }
                    AnthropicContentBlock::ToolUse { id, name, .. } => {
                        while content.len() <= idx {
                            content.push(Content::ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: serde_json::Value::Object(Default::default()),
                            });
                        }
                        let _ = tx.send(StreamEvent::ToolCallStart {
                            content_index: idx,
                            id,
                            name,
                        });
                    }
                }
            }
        }
        "content_block_delta" => {
            if let Ok(data) = serde_json::from_str::<AnthropicContentBlockDelta>(&sse.data) {
                let idx = data.index as usize;
                match data.delta {
                    AnthropicDelta::TextDelta { text } => {
                        if let Some(Content::Text { text: ref mut t }) = content.get_mut(idx) {
                            t.push_str(&text);
                        }
                        let _ = tx.send(StreamEvent::TextDelta {
                            content_index: idx,
                            delta: text,
                        });
                    }
                    AnthropicDelta::ThinkingDelta { thinking } => {
                        if let Some(Content::Thinking {
                            thinking: ref mut t,
                            ..
                        }) = content.get_mut(idx)
                        {
                            t.push_str(&thinking);
                        }
                        let _ = tx.send(StreamEvent::ThinkingDelta {
                            content_index: idx,
                            delta: thinking,
                        });
                    }
                    AnthropicDelta::InputJsonDelta { partial_json } => {
                        if let Some(Content::ToolCall {
                            ref mut arguments, ..
                        }) = content.get_mut(idx)
                        {
                            let buf = arguments
                                .as_object_mut()
                                .and_then(|o| o.get_mut("__partial_json"))
                                .and_then(|v| v.as_str().map(|s| s.to_string()));
                            let new_buf = format!("{}{}", buf.unwrap_or_default(), partial_json);
                            if let Some(obj) = arguments.as_object_mut() {
                                obj.insert(
                                    "__partial_json".into(),
                                    serde_json::Value::String(new_buf),
                                );
                            }
                        }
                        let _ = tx.send(StreamEvent::ToolCallDelta {
                            content_index: idx,
                            delta: partial_json,
                        });
                    }
                    AnthropicDelta::SignatureDelta { signature } => {
                        if let Some(Content::Thinking {
                            signature: ref mut s,
                            ..
                        }) = content.get_mut(idx)
                        {
                            *s = Some(signature);
                        }
                    }
                }
            }
        }
        "content_block_stop" => {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&sse.data) {
                let idx = data["index"].as_u64().unwrap_or(0) as usize;
                // Parse accumulated JSON for tool calls
                if let Some(Content::ToolCall {
                    ref mut arguments, ..
                }) = content.get_mut(idx)
                {
                    if let Some(partial) = arguments
                        .as_object()
                        .and_then(|o| o.get("__partial_json"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                    {
                        match crate::provider::json_repair::try_repair_json(&partial) {
                            Ok(parsed) => *arguments = parsed,
                            Err(e) => {
                                debug!("Failed to parse tool call JSON: {} ({})", partial, e);
                                *arguments = serde_json::Value::Object(Default::default());
                            }
                        }
                    }
                }
                let _ = tx.send(StreamEvent::ToolCallEnd { content_index: idx });
            }
        }
        "message_delta" => {
            if let Ok(data) = serde_json::from_str::<AnthropicMessageDelta>(&sse.data) {
                *stop_reason = match data.delta.stop_reason.as_deref() {
                    Some("tool_use") => StopReason::ToolUse,
                    Some("max_tokens") => StopReason::Length,
                    _ => StopReason::Stop,
                };
                usage.output = data.usage.output_tokens;
            }
        }
        "message_stop" => {
            return Ok(Some(true));
        }
        "ping" | "message" => {}
        "error" => {
            let provider_err = classify_sse_error_event(&sse.data);
            return Err(provider_err);
        }
        other => {
            debug!("Unknown Anthropic event: {}", other);
        }
    }
    Ok(None)
}
