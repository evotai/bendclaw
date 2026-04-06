use super::super::model::run::AssistantBlock;
use super::super::model::run::UsageSummary;
use super::super::model::transcript::ToolCallRecord;
use super::super::model::transcript::TranscriptItem;

/// Extract text content from engine Content blocks.
pub fn extract_content_text(content: &[bend_engine::Content]) -> String {
    content
        .iter()
        .filter_map(|c| {
            if let bend_engine::Content::Text { text } = c {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert engine AgentMessages to TranscriptItems.
pub fn from_agent_messages(messages: &[bend_engine::AgentMessage]) -> Vec<TranscriptItem> {
    messages.iter().map(transcript_from_agent_message).collect()
}

/// Convert TranscriptItems to engine AgentMessages.
pub fn into_agent_messages(items: &[TranscriptItem]) -> Vec<bend_engine::AgentMessage> {
    items.iter().map(agent_message_from_transcript).collect()
}

/// Convert a single engine AgentMessage to a TranscriptItem.
pub fn transcript_from_agent_message(message: &bend_engine::AgentMessage) -> TranscriptItem {
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

/// Convert a single TranscriptItem to an engine AgentMessage.
pub fn agent_message_from_transcript(item: &TranscriptItem) -> bend_engine::AgentMessage {
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

/// Convert engine Content blocks to AssistantBlocks (for ProtocolEvent).
pub fn assistant_blocks_from_content(content: &[bend_engine::Content]) -> Vec<AssistantBlock> {
    content
        .iter()
        .filter_map(|block| match block {
            bend_engine::Content::Text { text } => {
                Some(AssistantBlock::Text { text: text.clone() })
            }
            bend_engine::Content::Thinking { thinking, .. } => Some(AssistantBlock::Thinking {
                text: thinking.clone(),
            }),
            bend_engine::Content::ToolCall {
                id,
                name,
                arguments,
            } => Some(AssistantBlock::ToolCall {
                id: id.clone(),
                name: name.clone(),
                input: arguments.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Compute total usage from engine AgentMessages.
pub fn total_usage(messages: &[bend_engine::AgentMessage]) -> UsageSummary {
    let mut input: u64 = 0;
    let mut output: u64 = 0;

    for message in messages {
        if let bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant { usage, .. }) =
            message
        {
            input += usage.input;
            output += usage.output;
        }
    }

    UsageSummary { input, output }
}

/// Extract the last assistant text from engine AgentMessages.
pub fn extract_last_assistant_text(messages: &[bend_engine::AgentMessage]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|message| {
            if let bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
                content, ..
            }) = message
            {
                let text = extract_content_text(content);
                if !text.is_empty() {
                    return Some(text);
                }
            }
            None
        })
        .unwrap_or_default()
}
