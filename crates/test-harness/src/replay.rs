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
use serde::Deserialize;
use serde::Serialize;

pub use crate::mocks::llm::MockTurn;

// ── Token usage ───────────────────────────────────────────────────────────────

fn mock_usage_from(u: Option<&FixtureUsage>) -> TokenUsage {
    match u {
        Some(u) => TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.prompt_tokens + u.completion_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        },
        None => TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        },
    }
}

// ── Fixture types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReplayFixture {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub exchanges: Vec<FixtureExchange>,
    #[serde(default)]
    pub expectations: TraceExpectations,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FixtureExchange {
    pub response: FixtureTurn,
    #[serde(default)]
    pub usage: Option<FixtureUsage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FixtureTurn {
    Text { content: String },
    ToolCall { name: String, arguments: String },
    ToolCalls { calls: Vec<FixtureCall> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FixtureCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FixtureUsage {
    #[serde(default)]
    pub prompt_tokens: i64,
    #[serde(default)]
    pub completion_tokens: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TraceExpectations {
    #[serde(default)]
    pub response_contains: Vec<String>,
    #[serde(default)]
    pub response_matches: Vec<String>,
    #[serde(default)]
    pub tools_used: Vec<String>,
    pub max_tool_calls: Option<usize>,
    pub max_exchanges: Option<usize>,
}

// ── CallRecord ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CallRecord {
    pub turn: MockTurn,
    pub usage: TokenUsage,
}

// ── TraceLlm ──────────────────────────────────────────────────────────────────

/// Replay-based LLM provider for deterministic testing.
///
/// Loads a fixture file and replays exchanges in order. When only one exchange
/// remains it is reused for all subsequent calls (same as MockLLMProvider).
///
/// After the test, call `verify()` to check declarative expectations.
pub struct TraceLlm {
    exchanges: Mutex<VecDeque<FixtureExchange>>,
    call_log: Mutex<Vec<CallRecord>>,
    expectations: TraceExpectations,
}

impl TraceLlm {
    /// Load from a replay fixture file in `tests/fixtures/replays/`.
    pub fn from_replay(name: &str) -> anyhow::Result<Self> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/replays")
            .join(format!("{name}.json"));
        let raw = std::fs::read_to_string(&path).with_context(|| {
            format!("replay fixture {name}.json not found at {}", path.display())
        })?;
        let fixture: ReplayFixture = serde_json::from_str(&raw)
            .with_context(|| format!("replay fixture {name}.json invalid JSON"))?;
        Ok(Self {
            exchanges: Mutex::new(VecDeque::from(fixture.exchanges)),
            call_log: Mutex::new(Vec::new()),
            expectations: fixture.expectations,
        })
    }

    /// Build directly from a list of turns (no file needed).
    pub fn from_turns(turns: Vec<MockTurn>) -> Self {
        let exchanges = turns
            .into_iter()
            .map(|t| FixtureExchange {
                response: mock_turn_to_fixture(t),
                usage: None,
            })
            .collect();
        Self {
            exchanges: Mutex::new(exchanges),
            call_log: Mutex::new(Vec::new()),
            expectations: TraceExpectations::default(),
        }
    }

    /// Verify all declared expectations against the recorded call log.
    pub fn verify(&self) -> anyhow::Result<()> {
        let log = self.call_log.lock();
        let exp = &self.expectations;

        // max_exchanges
        if let Some(max) = exp.max_exchanges {
            if log.len() > max {
                bail!("expected at most {max} LLM exchanges, got {}", log.len());
            }
        }

        // tools_used
        let tools_called: Vec<String> = log
            .iter()
            .flat_map(|r| match &r.turn {
                MockTurn::ToolCall { name, .. } => vec![name.clone()],
                MockTurn::ToolCalls(calls) => calls.iter().map(|(n, _)| n.clone()).collect(),
                MockTurn::Text(_) => vec![],
            })
            .collect();

        for expected_tool in &exp.tools_used {
            if !tools_called.contains(expected_tool) {
                bail!("expected tool {expected_tool:?} to be called, got: {tools_called:?}");
            }
        }

        // max_tool_calls
        if let Some(max) = exp.max_tool_calls {
            if tools_called.len() > max {
                bail!(
                    "expected at most {max} tool calls, got {}",
                    tools_called.len()
                );
            }
        }

        // response_contains / response_matches checked against last text response
        let last_text = log.iter().rev().find_map(|r| {
            if let MockTurn::Text(t) = &r.turn {
                Some(t.clone())
            } else {
                None
            }
        });

        for substr in &exp.response_contains {
            match &last_text {
                Some(t) if t.contains(substr.as_str()) => {}
                Some(t) => bail!("expected response to contain {substr:?}, got: {t:?}"),
                None => {
                    bail!("expected response to contain {substr:?}, but no text response found")
                }
            }
        }

        for pattern in &exp.response_matches {
            let re = regex::Regex::new(pattern)
                .with_context(|| format!("invalid regex pattern: {pattern}"))?;
            match &last_text {
                Some(t) if re.is_match(t) => {}
                Some(t) => bail!("expected response to match /{pattern}/, got: {t:?}"),
                None => bail!("expected response to match /{pattern}/, but no text response found"),
            }
        }

        Ok(())
    }

    /// Return a copy of the call log.
    pub fn call_log(&self) -> Vec<CallRecord> {
        self.call_log.lock().clone()
    }

    fn next_exchange(&self) -> FixtureExchange {
        let mut q = self.exchanges.lock();
        if q.len() > 1 {
            q.pop_front().expect("len > 1")
        } else {
            q.front().cloned().unwrap_or(FixtureExchange {
                response: FixtureTurn::Text {
                    content: String::new(),
                },
                usage: None,
            })
        }
    }

    fn record(&self, turn: MockTurn, usage: TokenUsage) {
        self.call_log.lock().push(CallRecord { turn, usage });
    }
}

#[async_trait]
impl LLMProvider for TraceLlm {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> bendclaw::base::Result<LLMResponse> {
        let ex = self.next_exchange();
        let usage = mock_usage_from(ex.usage.as_ref());
        let (turn, resp) = fixture_turn_to_response(ex.response, usage.clone());
        self.record(turn, usage);
        Ok(resp)
    }

    fn chat_stream(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> ResponseStream {
        let ex = self.next_exchange();
        let usage = mock_usage_from(ex.usage.as_ref());
        let (turn, _) = fixture_turn_to_response(ex.response.clone(), usage.clone());
        self.record(turn, usage.clone());

        let (writer, stream) = ResponseStream::channel(16);
        tokio::spawn(async move {
            emit_stream_events(&writer, ex.response, usage).await;
        });
        stream
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn mock_turn_to_fixture(t: MockTurn) -> FixtureTurn {
    match t {
        MockTurn::Text(s) => FixtureTurn::Text { content: s },
        MockTurn::ToolCall { name, arguments } => FixtureTurn::ToolCall { name, arguments },
        MockTurn::ToolCalls(calls) => FixtureTurn::ToolCalls {
            calls: calls
                .into_iter()
                .map(|(name, arguments)| FixtureCall { name, arguments })
                .collect(),
        },
    }
}

fn fixture_turn_to_response(turn: FixtureTurn, usage: TokenUsage) -> (MockTurn, LLMResponse) {
    match turn {
        FixtureTurn::Text { content } => (MockTurn::Text(content.clone()), LLMResponse {
            content: Some(content),
            tool_calls: vec![],
            finish_reason: Some("stop".into()),
            usage: Some(usage),
            model: Some("trace".into()),
        }),
        FixtureTurn::ToolCall { name, arguments } => (
            MockTurn::ToolCall {
                name: name.clone(),
                arguments: arguments.clone(),
            },
            LLMResponse {
                content: None,
                tool_calls: vec![ToolCall {
                    id: "tc_trace_001".into(),
                    name,
                    arguments,
                }],
                finish_reason: Some("tool_calls".into()),
                usage: Some(usage),
                model: Some("trace".into()),
            },
        ),
        FixtureTurn::ToolCalls { calls } => {
            let mock_calls: Vec<(String, String)> = calls
                .iter()
                .map(|c| (c.name.clone(), c.arguments.clone()))
                .collect();
            let tool_calls = calls
                .into_iter()
                .enumerate()
                .map(|(i, c)| ToolCall {
                    id: format!("tc_trace_{i:03}"),
                    name: c.name,
                    arguments: c.arguments,
                })
                .collect();
            (MockTurn::ToolCalls(mock_calls), LLMResponse {
                content: None,
                tool_calls,
                finish_reason: Some("tool_calls".into()),
                usage: Some(usage),
                model: Some("trace".into()),
            })
        }
    }
}

async fn emit_stream_events(
    writer: &bendclaw::llm::stream::StreamWriter,
    turn: FixtureTurn,
    usage: TokenUsage,
) {
    match turn {
        FixtureTurn::Text { content } => {
            writer.send(StreamEvent::ContentDelta(content)).await;
        }
        FixtureTurn::ToolCall { name, arguments } => {
            let id = "tc_trace_001".to_string();
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
        FixtureTurn::ToolCalls { calls } => {
            for (i, c) in calls.into_iter().enumerate() {
                let id = format!("tc_trace_{i:03}");
                writer
                    .send(StreamEvent::ToolCallStart {
                        index: i,
                        id: id.clone(),
                        name: c.name.clone(),
                    })
                    .await;
                writer
                    .send(StreamEvent::ToolCallEnd {
                        index: i,
                        id,
                        name: c.name,
                        arguments: c.arguments,
                    })
                    .await;
            }
        }
    }
    writer.send(StreamEvent::Usage(usage)).await;
    writer
        .send(StreamEvent::Done {
            finish_reason: "stop".into(),
            provider: Some("trace".into()),
            model: Some("trace".into()),
        })
        .await;
}
