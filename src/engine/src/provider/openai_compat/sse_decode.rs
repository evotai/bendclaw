//! OpenAI-compatible SSE stream decoding.
//!
//! Parses OpenAI Chat Completions streaming chunks and translates them
//! into internal [`StreamEvent`]s while accumulating the final [`Message`].

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::request::ToolCallBuffer;
use super::types::*;
use crate::provider::error::ProviderError;
use crate::provider::model::OpenAiCompat;
use crate::provider::model::ThinkingFormat;
use crate::provider::sse::SseEvent;
use crate::provider::stream_http;
use crate::provider::traits::StreamConfig;
use crate::provider::traits::StreamEvent;
use crate::types::*;

/// Drive an OpenAI-compatible SSE stream from a raw HTTP response.
pub(crate) async fn decode_sse_stream(
    response: reqwest::Response,
    tx: mpsc::UnboundedSender<StreamEvent>,
    cancel: CancellationToken,
    config: &StreamConfig,
    compat: &OpenAiCompat,
) -> Result<Message, ProviderError> {
    let (sse_tx, mut sse_rx) = mpsc::unbounded_channel::<SseEvent>();

    let sse_cancel = cancel.clone();
    let sse_handle =
        tokio::spawn(
            async move { stream_http::drive_sse_response(response, sse_tx, sse_cancel).await },
        );

    let mut content: Vec<Content> = Vec::new();
    let mut usage = Usage::default();
    let mut stop_reason = StopReason::Stop;
    let mut tool_call_buffers: Vec<ToolCallBuffer> = Vec::new();

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
                        if sse.data == "[DONE]" {
                            break;
                        }
                        process_sse_chunk(
                            &sse,
                            &tx,
                            &mut content,
                            &mut usage,
                            &mut stop_reason,
                            &mut tool_call_buffers,
                            compat,
                        )?;
                    }
                }
            }
        }
    }

    // Wait for SSE driver to finish
    if let Ok(Err(e)) = sse_handle.await {
        if content.is_empty() && tool_call_buffers.is_empty() {
            return Err(ProviderError::Network(e));
        }
        debug!("SSE driver ended with error after content received: {e}");
    }

    // Detect empty response: no content and no usage from provider
    if content.is_empty() && tool_call_buffers.is_empty() && usage.input == 0 && usage.output == 0 {
        return Err(ProviderError::Api(
            "Empty response from provider (no content, no usage)".into(),
        ));
    }

    // Finalize tool calls
    finalize_tool_calls(&tx, &mut content, &tool_call_buffers);

    if !tool_call_buffers.is_empty()
        || content
            .iter()
            .any(|c| matches!(c, Content::ToolCall { .. }))
    {
        stop_reason = StopReason::ToolUse;
    }

    let message = Message::Assistant {
        content,
        stop_reason,
        model: config.model.clone(),
        provider: config
            .model_config
            .as_ref()
            .map(|mc| mc.provider.clone())
            .unwrap_or_else(|| "openai".into()),
        usage,
        timestamp: now_ms(),
        error_message: None,
    };

    let _ = tx.send(StreamEvent::Done {
        message: message.clone(),
    });
    Ok(message)
}

fn process_sse_chunk(
    sse: &SseEvent,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    content: &mut Vec<Content>,
    usage: &mut Usage,
    stop_reason: &mut StopReason,
    tool_call_buffers: &mut Vec<ToolCallBuffer>,
    compat: &OpenAiCompat,
) -> Result<(), ProviderError> {
    let chunk: OpenAiChunk = match serde_json::from_str(&sse.data) {
        Ok(c) => c,
        Err(e) => {
            debug!("Failed to parse OpenAI chunk: {} data={}", e, &sse.data);
            return Ok(());
        }
    };

    // Check for inline error (non-standard but used by some proxies)
    if let Some(err) = &chunk.error {
        let msg = if err.message.is_empty() {
            sse.data.clone()
        } else {
            err.message.clone()
        };
        debug!("OpenAI stream error: {}", msg);
        return Err(ProviderError::Api(msg));
    }

    // Process usage
    if let Some(u) = &chunk.usage {
        usage.input = u.prompt_tokens;
        usage.output = u.completion_tokens;
        usage.total_tokens = u.total_tokens;
        if let Some(details) = &u.prompt_tokens_details {
            usage.cache_read = details.cached_tokens;
        }
    }

    for choice in &chunk.choices {
        let delta = &choice.delta;

        // Handle reasoning/thinking content
        let reasoning = match compat.thinking_format {
            ThinkingFormat::Xai => delta.reasoning.as_deref(),
            _ => delta.reasoning_content.as_deref(),
        };
        if let Some(reasoning_text) = reasoning {
            let thinking_idx = content
                .iter()
                .position(|c| matches!(c, Content::Thinking { .. }));
            let idx = match thinking_idx {
                Some(i) => i,
                None => {
                    content.push(Content::Thinking {
                        thinking: String::new(),
                        signature: None,
                    });
                    content.len() - 1
                }
            };
            if let Some(Content::Thinking { thinking, .. }) = content.get_mut(idx) {
                thinking.push_str(reasoning_text);
            }
            let _ = tx.send(StreamEvent::ThinkingDelta {
                content_index: idx,
                delta: reasoning_text.to_string(),
            });
        }

        // Handle text content
        if let Some(text) = &delta.content {
            let text_idx = content
                .iter()
                .position(|c| matches!(c, Content::Text { .. }));
            let idx = match text_idx {
                Some(i) => i,
                None => {
                    content.push(Content::Text {
                        text: String::new(),
                    });
                    content.len() - 1
                }
            };
            if let Some(Content::Text { text: t }) = content.get_mut(idx) {
                t.push_str(text);
            }
            let _ = tx.send(StreamEvent::TextDelta {
                content_index: idx,
                delta: text.clone(),
            });
        }

        // Handle tool calls
        if let Some(tool_calls) = &delta.tool_calls {
            for tc in tool_calls {
                let tc_index = tc.index as usize;
                while tool_call_buffers.len() <= tc_index {
                    tool_call_buffers.push(ToolCallBuffer::default());
                }
                let buf = &mut tool_call_buffers[tc_index];
                if let Some(id) = &tc.id {
                    buf.id = id.clone();
                }
                if let Some(f) = &tc.function {
                    if let Some(name) = &f.name {
                        buf.name.clone_from(name);
                        let _ = tx.send(StreamEvent::ToolCallStart {
                            content_index: content.len() + tc_index,
                            id: buf.id.clone(),
                            name: name.clone(),
                        });
                    }
                    if let Some(args) = &f.arguments {
                        buf.arguments.push_str(args);
                        let _ = tx.send(StreamEvent::ToolCallDelta {
                            content_index: content.len() + tc_index,
                            delta: args.clone(),
                        });
                    }
                }
            }
        }

        // Handle finish reason
        if let Some(reason) = &choice.finish_reason {
            *stop_reason = match reason.as_str() {
                "stop" => StopReason::Stop,
                "length" => StopReason::Length,
                "tool_calls" => StopReason::ToolUse,
                _ => StopReason::Stop,
            };
        }
    }

    Ok(())
}

fn finalize_tool_calls(
    tx: &mpsc::UnboundedSender<StreamEvent>,
    content: &mut Vec<Content>,
    tool_call_buffers: &[ToolCallBuffer],
) {
    for buf in tool_call_buffers.iter() {
        let args = crate::provider::json_repair::try_repair_json(&buf.arguments)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        content.push(Content::ToolCall {
            id: buf.id.clone(),
            name: buf.name.clone(),
            arguments: args,
        });
        let _ = tx.send(StreamEvent::ToolCallEnd {
            content_index: content.len() - 1,
        });
    }
}
