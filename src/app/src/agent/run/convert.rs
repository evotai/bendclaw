//! Message conversion — between engine AgentMessages and TranscriptItems.

use crate::types::AssistantBlock;
use crate::types::ToolCallRecord;
use crate::types::TranscriptImageSource;
use crate::types::TranscriptItem;
use crate::types::TranscriptUserContent;
use crate::types::UsageSummary;

/// Extract text content from engine Content blocks.
pub fn extract_content_text(content: &[evot_engine::Content]) -> String {
    content
        .iter()
        .filter_map(|c| {
            if let evot_engine::Content::Text { text } = c {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert engine AgentMessages to TranscriptItems.
pub fn from_agent_messages(messages: &[evot_engine::AgentMessage]) -> Vec<TranscriptItem> {
    messages.iter().map(transcript_from_agent_message).collect()
}

/// Convert TranscriptItems to engine AgentMessages.
pub fn into_agent_messages(items: &[TranscriptItem]) -> Vec<evot_engine::AgentMessage> {
    items.iter().map(agent_message_from_transcript).collect()
}

/// Convert a single engine AgentMessage to a TranscriptItem.
pub fn transcript_from_agent_message(message: &evot_engine::AgentMessage) -> TranscriptItem {
    match message {
        evot_engine::AgentMessage::Llm(evot_engine::Message::User { content, .. }) => {
            TranscriptItem::user_from_content(content)
        }
        evot_engine::AgentMessage::Llm(evot_engine::Message::Assistant {
            content,
            stop_reason,
            ..
        }) => {
            let mut text = String::new();
            let mut thinking = None;
            let mut tool_calls = Vec::new();

            for block in content {
                match block {
                    evot_engine::Content::Text { text: chunk } => {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(chunk);
                    }
                    evot_engine::Content::Thinking {
                        thinking: chunk, ..
                    } => {
                        thinking = Some(chunk.clone());
                    }
                    evot_engine::Content::ToolCall {
                        id,
                        name,
                        arguments,
                    } => {
                        tool_calls.push(ToolCallRecord {
                            id: id.clone(),
                            name: name.clone(),
                            input: scrub_tool_args(name, arguments),
                        });
                    }
                    _ => {}
                }
            }

            TranscriptItem::Assistant {
                text,
                thinking,
                tool_calls,
                stop_reason: stop_reason.to_string(),
            }
        }
        evot_engine::AgentMessage::Llm(evot_engine::Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        }) => {
            let text = extract_content_text(content);
            TranscriptItem::ToolResult {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                content: text,
                is_error: *is_error,
            }
        }
        evot_engine::AgentMessage::Extension(ext) => TranscriptItem::Extension {
            kind: ext.kind.clone(),
            data: ext.data.clone(),
        },
    }
}

/// Convert a single TranscriptItem to an engine AgentMessage.
pub fn agent_message_from_transcript(item: &TranscriptItem) -> evot_engine::AgentMessage {
    match item {
        TranscriptItem::User { text, content } => {
            let content = if content.is_empty() {
                vec![evot_engine::Content::Text { text: text.clone() }]
            } else {
                content
                    .iter()
                    .map(|item| match item {
                        TranscriptUserContent::Text { text } => {
                            evot_engine::Content::Text { text: text.clone() }
                        }
                        TranscriptUserContent::Image { mime_type, source } => {
                            let source = match source {
                                TranscriptImageSource::Path { path } => {
                                    evot_engine::ImageSource::Path { path: path.clone() }
                                }
                                TranscriptImageSource::Base64 { data } => {
                                    evot_engine::ImageSource::Base64 { data: data.clone() }
                                }
                            };
                            evot_engine::Content::Image {
                                mime_type: mime_type.clone(),
                                source,
                            }
                        }
                    })
                    .collect()
            };
            evot_engine::AgentMessage::Llm(evot_engine::Message::User {
                content,
                timestamp: evot_engine::now_ms(),
            })
        }
        TranscriptItem::Assistant {
            text,
            thinking,
            tool_calls,
            stop_reason,
        } => {
            let mut content = Vec::new();

            if let Some(thinking) = thinking {
                content.push(evot_engine::Content::Thinking {
                    thinking: thinking.clone(),
                    signature: None,
                });
            }
            if !text.is_empty() {
                content.push(evot_engine::Content::Text { text: text.clone() });
            }
            for tool_call in tool_calls {
                content.push(evot_engine::Content::ToolCall {
                    id: tool_call.id.clone(),
                    name: tool_call.name.clone(),
                    arguments: tool_call.input.clone(),
                });
            }

            evot_engine::AgentMessage::Llm(evot_engine::Message::Assistant {
                content,
                stop_reason: parse_stop_reason(stop_reason),
                model: String::new(),
                provider: String::new(),
                usage: evot_engine::Usage::default(),
                timestamp: evot_engine::types::now_ms(),
                error_message: None,
                response_id: None,
            })
        }
        TranscriptItem::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
        } => evot_engine::AgentMessage::Llm(evot_engine::Message::ToolResult {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            content: vec![evot_engine::Content::Text {
                text: content.clone(),
            }],
            is_error: *is_error,
            timestamp: evot_engine::types::now_ms(),
            retention: evot_engine::Retention::Normal,
        }),
        TranscriptItem::System { text } => evot_engine::AgentMessage::Extension(
            evot_engine::ExtensionMessage::new("system", serde_json::json!({ "text": text })),
        ),
        TranscriptItem::Extension { kind, data } => evot_engine::AgentMessage::Extension(
            evot_engine::ExtensionMessage::new(kind.clone(), data.clone()),
        ),
        TranscriptItem::Compact { .. } => evot_engine::AgentMessage::Extension(
            evot_engine::ExtensionMessage::new("compact", serde_json::json!({})),
        ),
        // Marker items should never reach conversion — filtered by resolve_transcript.
        // Defensive fallback: convert to a no-op extension that the engine will ignore.
        TranscriptItem::Marker { .. } => evot_engine::AgentMessage::Extension(
            evot_engine::ExtensionMessage::new("marker", serde_json::json!({})),
        ),
        // Stats items should never reach conversion — filtered by resolve_transcript.
        // Defensive fallback: convert to a no-op extension that the engine will ignore.
        TranscriptItem::Stats { .. } => evot_engine::AgentMessage::Extension(
            evot_engine::ExtensionMessage::new("internal_stats", serde_json::json!({})),
        ),
    }
}

/// Convert engine Content blocks to AssistantBlocks (for ProtocolEvent).
pub fn assistant_blocks_from_content(content: &[evot_engine::Content]) -> Vec<AssistantBlock> {
    content
        .iter()
        .filter_map(|block| match block {
            evot_engine::Content::Text { text } => {
                Some(AssistantBlock::Text { text: text.clone() })
            }
            evot_engine::Content::Thinking { thinking, .. } => Some(AssistantBlock::Thinking {
                text: thinking.clone(),
            }),
            evot_engine::Content::ToolCall {
                id,
                name,
                arguments,
            } => Some(AssistantBlock::ToolCall {
                id: id.clone(),
                name: name.clone(),
                input: scrub_tool_args(name, arguments),
            }),
            _ => None,
        })
        .collect()
}

/// Compute total usage from engine AgentMessages.
pub fn total_usage(messages: &[evot_engine::AgentMessage]) -> UsageSummary {
    let mut input: u64 = 0;
    let mut output: u64 = 0;
    let mut cache_read: u64 = 0;
    let mut cache_write: u64 = 0;

    for message in messages {
        if let evot_engine::AgentMessage::Llm(evot_engine::Message::Assistant { usage, .. }) =
            message
        {
            input += usage.input;
            output += usage.output;
            cache_read += usage.cache_read;
            cache_write += usage.cache_write;
        }
    }

    UsageSummary {
        input,
        output,
        cache_read,
        cache_write,
    }
}

pub fn scrub_tool_args(_tool_name: &str, args: &serde_json::Value) -> serde_json::Value {
    args.clone()
}

/// Parse a stop_reason string back into the engine StopReason enum.
fn parse_stop_reason(s: &str) -> evot_engine::StopReason {
    match s {
        "stop" => evot_engine::StopReason::Stop,
        "length" => evot_engine::StopReason::Length,
        "toolUse" => evot_engine::StopReason::ToolUse,
        "error" => evot_engine::StopReason::Error,
        "aborted" => evot_engine::StopReason::Aborted,
        _ => evot_engine::StopReason::Stop,
    }
}

/// Build a TranscriptItem::Assistant from AssistantBlock content and stop_reason.
/// Used by the app event loop to incrementally build transcripts from ProtocolEvents.
pub fn transcript_from_assistant_completed(
    content: &[AssistantBlock],
    stop_reason: &str,
) -> TranscriptItem {
    let mut text = String::new();
    let mut thinking = None;
    let mut tool_calls = Vec::new();

    for block in content {
        match block {
            AssistantBlock::Text { text: t } => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(t);
            }
            AssistantBlock::Thinking { text: t } => {
                thinking = Some(t.clone());
            }
            AssistantBlock::ToolCall { id, name, input } => {
                tool_calls.push(ToolCallRecord {
                    id: id.clone(),
                    name: name.clone(),
                    input: scrub_tool_args(name, input),
                });
            }
        }
    }

    TranscriptItem::Assistant {
        text,
        thinking,
        tool_calls,
        stop_reason: stop_reason.to_string(),
    }
}
