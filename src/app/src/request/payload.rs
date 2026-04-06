use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultPayload {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestFinishedPayload {
    pub text: String,
    pub usage: Value,
    pub turn_count: u32,
    pub duration_ms: u64,
    pub transcript_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantPayload {
    pub content: Vec<AssistantBlock>,
    pub usage: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantBlock {
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    Thinking {
        text: String,
    },
}

pub fn payload_as<T: DeserializeOwned>(payload: &Value) -> Option<T> {
    serde_json::from_value(payload.clone()).ok()
}

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

impl From<&bend_engine::AgentMessage> for AssistantPayload {
    fn from(message: &bend_engine::AgentMessage) -> Self {
        let (content, usage) = match message {
            bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
                content,
                usage,
                ..
            }) => {
                let blocks: Vec<AssistantBlock> = content
                    .iter()
                    .filter_map(|block| match block {
                        bend_engine::Content::Text { text } => {
                            Some(AssistantBlock::Text { text: text.clone() })
                        }
                        bend_engine::Content::Thinking { thinking, .. } => {
                            Some(AssistantBlock::Thinking {
                                text: thinking.clone(),
                            })
                        }
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
                    .collect();
                let usage = serde_json::to_value(usage).ok();
                (blocks, usage)
            }
            _ => (vec![], None),
        };

        Self { content, usage }
    }
}

impl ToolResultPayload {
    pub fn from_result(
        tool_call_id: &str,
        tool_name: &str,
        result: &bend_engine::ToolResult,
        is_error: bool,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            content: extract_content_text(&result.content),
            is_error,
        }
    }
}

impl RequestFinishedPayload {
    pub fn from_messages(
        messages: &[bend_engine::AgentMessage],
        turn_count: u32,
        duration_ms: u64,
    ) -> Self {
        Self {
            text: extract_last_assistant_text(messages),
            usage: total_usage(messages),
            turn_count,
            duration_ms,
            transcript_count: messages.len(),
        }
    }
}

fn extract_last_assistant_text(messages: &[bend_engine::AgentMessage]) -> String {
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

fn total_usage(messages: &[bend_engine::AgentMessage]) -> Value {
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

    serde_json::json!({ "input": input, "output": output })
}
