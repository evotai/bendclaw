use std::collections::VecDeque;

use anyhow::bail;
use anyhow::Context as _;
use async_trait::async_trait;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::message::ToolCall;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::stream::StreamEvent;
use bendclaw::llm::tool::ToolSchema;
use bendclaw::llm::usage::TokenUsage;
use parking_lot::Mutex;

fn mock_usage() -> TokenUsage {
    TokenUsage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    }
}

/// A single turn in a mock conversation.
#[derive(Clone, Debug)]
pub enum MockTurn {
    /// Return a text response.
    Text(String),
    /// Return a single tool call.
    ToolCall { name: String, arguments: String },
    /// Return multiple tool calls.
    ToolCalls(Vec<(String, String)>),
}

/// Mock LLM provider that supports multi-turn conversations.
///
/// Each `chat()` / `chat_stream()` call pops the next turn from the queue.
/// When only one turn remains, it is reused for all subsequent calls.
pub struct MockLLMProvider {
    turns: Mutex<VecDeque<MockTurn>>,
}

impl MockLLMProvider {
    /// Create a provider with a sequence of turns.
    pub fn new(turns: Vec<MockTurn>) -> Self {
        Self {
            turns: Mutex::new(VecDeque::from(turns)),
        }
    }

    /// Shorthand: always returns the same text.
    pub fn with_text(text: &str) -> Self {
        Self::new(vec![MockTurn::Text(text.to_string())])
    }

    /// Shorthand: always returns the same tool call.
    pub fn always_tool_call(name: &str, arguments: &str) -> Self {
        Self::new(vec![MockTurn::ToolCall {
            name: name.to_string(),
            arguments: arguments.to_string(),
        }])
    }

    /// Load turns from a JSON fixture file in `tests/fixtures/llm_traces/`.
    ///
    /// # Fixture format
    /// ```json
    /// { "turns": [
    ///   { "type": "text", "content": "hello" },
    ///   { "type": "tool_call", "name": "shell", "arguments": "{}" },
    ///   { "type": "tool_calls", "calls": [{"name": "a", "arguments": "{}"}] }
    /// ]}
    /// ```
    pub fn from_fixture(name: &str) -> anyhow::Result<Self> {
        let path = fixture_path(name)?;
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("fixture {name}.json not found at {}", path.display()))?;
        let doc: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("fixture {name}.json invalid JSON"))?;
        let turns = doc["turns"]
            .as_array()
            .with_context(|| format!("fixture {name}.json missing 'turns' array"))?
            .iter()
            .map(|t| parse_fixture_turn(t, name))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self::new(turns))
    }

    fn next_turn(&self) -> MockTurn {
        let mut turns = self.turns.lock();
        if turns.len() > 1 {
            turns.pop_front().expect("len > 1 guarantees front exists")
        } else {
            turns
                .front()
                .cloned()
                .unwrap_or(MockTurn::Text(String::new()))
        }
    }
}

fn fixture_path(name: &str) -> anyhow::Result<std::path::PathBuf> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("tests/fixtures/llm_traces"),
        manifest_dir.join("../../tests/fixtures/llm_traces"),
    ];

    candidates
        .iter()
        .map(|dir| dir.join(format!("{name}.json")))
        .find(|path| path.exists())
        .with_context(|| {
            format!(
                "fixture {name}.json not found in candidates: {}",
                candidates
                    .iter()
                    .map(|dir| dir.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

#[async_trait]
impl LLMProvider for MockLLMProvider {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> bendclaw::base::Result<LLMResponse> {
        let turn = self.next_turn();
        match turn {
            MockTurn::Text(text) => Ok(LLMResponse {
                content: Some(text),
                tool_calls: vec![],
                finish_reason: Some("stop".into()),
                usage: Some(mock_usage()),
                model: Some("mock".into()),
            }),
            MockTurn::ToolCall { name, arguments } => Ok(LLMResponse {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "tc_mock_001".into(),
                    name,
                    arguments,
                }],
                finish_reason: Some("tool_calls".into()),
                usage: Some(mock_usage()),
                model: Some("mock".into()),
            }),
            MockTurn::ToolCalls(calls) => Ok(LLMResponse {
                content: None,
                tool_calls: calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, (name, arguments))| ToolCall {
                        id: format!("tc_mock_{i:03}"),
                        name,
                        arguments,
                    })
                    .collect(),
                finish_reason: Some("tool_calls".into()),
                usage: Some(mock_usage()),
                model: Some("mock".into()),
            }),
        }
    }

    fn chat_stream(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> ResponseStream {
        let turn = self.next_turn();
        let (writer, stream) = ResponseStream::channel(16);

        tokio::spawn(async move {
            match turn {
                MockTurn::Text(text) => {
                    writer.send(StreamEvent::ContentDelta(text)).await;
                }
                MockTurn::ToolCall { name, arguments } => {
                    let id = "tc_mock_001".to_string();
                    writer
                        .send(StreamEvent::ToolCallStart {
                            index: 0,
                            id: id.clone(),
                            name: name.clone(),
                        })
                        .await;
                    writer
                        .send(StreamEvent::ToolCallEnd {
                            index: 0,
                            id,
                            name,
                            arguments,
                        })
                        .await;
                }
                MockTurn::ToolCalls(calls) => {
                    for (i, (name, arguments)) in calls.into_iter().enumerate() {
                        let id = format!("tc_mock_{i:03}");
                        writer
                            .send(StreamEvent::ToolCallStart {
                                index: i,
                                id: id.clone(),
                                name: name.clone(),
                            })
                            .await;
                        writer
                            .send(StreamEvent::ToolCallEnd {
                                index: i,
                                id,
                                name,
                                arguments,
                            })
                            .await;
                    }
                }
            }
            writer.send(StreamEvent::Usage(mock_usage())).await;
            writer
                .send(StreamEvent::Done {
                    finish_reason: "stop".into(),
                    provider: Some("mock".into()),
                    model: Some("mock".into()),
                })
                .await;
        });

        stream
    }
}

fn parse_fixture_turn(t: &serde_json::Value, fixture_name: &str) -> anyhow::Result<MockTurn> {
    match t["type"].as_str().unwrap_or("") {
        "text" => {
            let content = t["content"]
                .as_str()
                .with_context(|| format!("fixture {fixture_name}: text turn missing 'content'"))?;
            Ok(MockTurn::Text(content.to_string()))
        }
        "tool_call" => {
            let name = t["name"]
                .as_str()
                .with_context(|| format!("fixture {fixture_name}: tool_call missing 'name'"))?;
            let arguments = t["arguments"].as_str().with_context(|| {
                format!("fixture {fixture_name}: tool_call missing 'arguments'")
            })?;
            Ok(MockTurn::ToolCall {
                name: name.to_string(),
                arguments: arguments.to_string(),
            })
        }
        "tool_calls" => {
            let calls = t["calls"]
                .as_array()
                .with_context(|| format!("fixture {fixture_name}: tool_calls missing 'calls'"))?
                .iter()
                .map(|c| {
                    let name = c["name"]
                        .as_str()
                        .with_context(|| format!("fixture {fixture_name}: call missing 'name'"))?;
                    let arguments = c["arguments"].as_str().with_context(|| {
                        format!("fixture {fixture_name}: call missing 'arguments'")
                    })?;
                    Ok((name.to_string(), arguments.to_string()))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(MockTurn::ToolCalls(calls))
        }
        other => bail!("fixture {fixture_name}: unknown turn type {other:?}"),
    }
}
