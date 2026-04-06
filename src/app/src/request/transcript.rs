use super::payload::extract_content_text;
use crate::storage::model::ToolCallRecord;
use crate::storage::model::TranscriptItem;

pub fn from_agent_messages(messages: &[bend_engine::AgentMessage]) -> Vec<TranscriptItem> {
    messages.iter().map(TranscriptItem::from).collect()
}

pub fn into_agent_messages(items: &[TranscriptItem]) -> Vec<bend_engine::AgentMessage> {
    items.iter().map(bend_engine::AgentMessage::from).collect()
}

impl From<&bend_engine::AgentMessage> for TranscriptItem {
    fn from(message: &bend_engine::AgentMessage) -> Self {
        match message {
            bend_engine::AgentMessage::Llm(bend_engine::Message::User { content, .. }) => {
                let text = extract_content_text(content);
                TranscriptItem::User { text }
            }
            bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant { content, .. }) => {
                let mut text = String::new();
                let mut thinking = None;
                let mut tool_calls = Vec::new();

                for block in content {
                    match block {
                        bend_engine::Content::Text { text: chunk } => {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(chunk);
                        }
                        bend_engine::Content::Thinking {
                            thinking: chunk, ..
                        } => {
                            thinking = Some(chunk.clone());
                        }
                        bend_engine::Content::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            tool_calls.push(ToolCallRecord {
                                id: id.clone(),
                                name: name.clone(),
                                input: arguments.clone(),
                            });
                        }
                        _ => {}
                    }
                }

                TranscriptItem::Assistant {
                    text,
                    thinking,
                    tool_calls,
                }
            }
            bend_engine::AgentMessage::Llm(bend_engine::Message::ToolResult {
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
            bend_engine::AgentMessage::Extension(ext) => TranscriptItem::Extension {
                kind: ext.kind.clone(),
                data: ext.data.clone(),
            },
        }
    }
}

impl From<&TranscriptItem> for bend_engine::AgentMessage {
    fn from(item: &TranscriptItem) -> Self {
        match item {
            TranscriptItem::User { text } => {
                bend_engine::AgentMessage::Llm(bend_engine::Message::user(text.clone()))
            }
            TranscriptItem::Assistant {
                text,
                thinking,
                tool_calls,
            } => {
                let mut content = Vec::new();

                if let Some(thinking) = thinking {
                    content.push(bend_engine::Content::Thinking {
                        thinking: thinking.clone(),
                        signature: None,
                    });
                }
                if !text.is_empty() {
                    content.push(bend_engine::Content::Text { text: text.clone() });
                }
                for tool_call in tool_calls {
                    content.push(bend_engine::Content::ToolCall {
                        id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.input.clone(),
                    });
                }

                bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
                    content,
                    stop_reason: bend_engine::StopReason::Stop,
                    model: String::new(),
                    provider: String::new(),
                    usage: bend_engine::Usage::default(),
                    timestamp: bend_engine::types::now_ms(),
                    error_message: None,
                })
            }
            TranscriptItem::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
            } => bend_engine::AgentMessage::Llm(bend_engine::Message::ToolResult {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                content: vec![bend_engine::Content::Text {
                    text: content.clone(),
                }],
                is_error: *is_error,
                timestamp: bend_engine::types::now_ms(),
            }),
            TranscriptItem::System { text } => bend_engine::AgentMessage::Extension(
                bend_engine::ExtensionMessage::new("system", serde_json::json!({ "text": text })),
            ),
            TranscriptItem::Extension { kind, data } => bend_engine::AgentMessage::Extension(
                bend_engine::ExtensionMessage::new(kind.clone(), data.clone()),
            ),
        }
    }
}
