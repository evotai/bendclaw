//! Tests for the core agent loop using MockProvider.

use bendengine::agent_loop::agent_loop;
use bendengine::agent_loop::agent_loop_continue;
use bendengine::agent_loop::AgentLoopConfig;
use bendengine::provider::mock::*;
use bendengine::provider::MockProvider;
use bendengine::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn make_config(provider: MockProvider) -> AgentLoopConfig {
    AgentLoopConfig {
        provider: std::sync::Arc::new(provider),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_strategy: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig::default(),
        before_turn: None,
        after_turn: None,
        on_error: None,
        input_filters: vec![],
    }
}

fn collect_events(mut rx: mpsc::UnboundedReceiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

#[tokio::test]
async fn test_simple_text_response() {
    let provider = MockProvider::text("Hello, world!");
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "You are helpful.".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("Hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);

    // Should have: AgentStart, TurnStart, MessageStart(user), MessageEnd(user),
    //              MessageStart(assistant), MessageEnd(assistant), TurnEnd, AgentEnd
    let event_types: Vec<&str> = events
        .iter()
        .map(|e| match e {
            AgentEvent::AgentStart => "AgentStart",
            AgentEvent::AgentEnd { .. } => "AgentEnd",
            AgentEvent::TurnStart => "TurnStart",
            AgentEvent::TurnEnd { .. } => "TurnEnd",
            AgentEvent::MessageStart { .. } => "MessageStart",
            AgentEvent::MessageEnd { .. } => "MessageEnd",
            AgentEvent::MessageUpdate { .. } => "MessageUpdate",
            AgentEvent::ToolExecutionStart { .. } => "ToolExecStart",
            AgentEvent::ToolExecutionUpdate { .. } => "ToolExecUpdate",
            AgentEvent::ToolExecutionEnd { .. } => "ToolExecEnd",
            AgentEvent::ProgressMessage { .. } => "ProgressMessage",
            AgentEvent::InputRejected { .. } => "InputRejected",
            AgentEvent::LlmCallStart { .. } => "LlmCallStart",
            AgentEvent::LlmCallEnd { .. } => "LlmCallEnd",
            AgentEvent::ContextCompactionStart { .. } => "CompactionStart",
            AgentEvent::ContextCompactionEnd { .. } => "CompactionEnd",
        })
        .collect();

    assert!(event_types.contains(&"AgentStart"));
    assert!(event_types.contains(&"AgentEnd"));
    assert!(event_types.contains(&"TurnStart"));
    assert!(event_types.contains(&"TurnEnd"));

    // new_messages should contain user prompt + assistant response
    assert_eq!(new_messages.len(), 2);
    assert_eq!(new_messages[0].role(), "user");
    assert_eq!(new_messages[1].role(), "assistant");

    // Context should have both messages
    assert_eq!(context.messages.len(), 2);
}

#[tokio::test]
async fn test_tool_call_and_response() {
    // Mock: first call returns tool use, second returns text
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "test.txt"}),
        }]),
        MockResponse::Text("The file contains: hello".into()),
    ]);

    // Define a simple tool
    struct ReadFileTool;

    #[async_trait::async_trait]
    impl AgentTool for ReadFileTool {
        fn name(&self) -> &str {
            "read_file"
        }
        fn label(&self) -> &str {
            "Read File"
        }
        fn description(&self) -> &str {
            "Read a file"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            })
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                content: vec![Content::Text {
                    text: "hello".into(),
                }],
                details: serde_json::Value::Null,
            })
        }
    }

    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "You are helpful.".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ReadFileTool)],
    };

    let prompt = AgentMessage::Llm(Message::user("Read test.txt"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);

    let event_types: Vec<&str> = events
        .iter()
        .map(|e| match e {
            AgentEvent::AgentStart => "AgentStart",
            AgentEvent::AgentEnd { .. } => "AgentEnd",
            AgentEvent::TurnStart => "TurnStart",
            AgentEvent::TurnEnd { .. } => "TurnEnd",
            AgentEvent::MessageStart { .. } => "MessageStart",
            AgentEvent::MessageEnd { .. } => "MessageEnd",
            AgentEvent::MessageUpdate { .. } => "MessageUpdate",
            AgentEvent::ToolExecutionStart { .. } => "ToolExecStart",
            AgentEvent::ToolExecutionUpdate { .. } => "ToolExecUpdate",
            AgentEvent::ToolExecutionEnd { .. } => "ToolExecEnd",
            AgentEvent::ProgressMessage { .. } => "ProgressMessage",
            AgentEvent::InputRejected { .. } => "InputRejected",
            AgentEvent::LlmCallStart { .. } => "LlmCallStart",
            AgentEvent::LlmCallEnd { .. } => "LlmCallEnd",
            AgentEvent::ContextCompactionStart { .. } => "CompactionStart",
            AgentEvent::ContextCompactionEnd { .. } => "CompactionEnd",
        })
        .collect();

    // Should have tool execution events
    assert!(event_types.contains(&"ToolExecStart"));
    assert!(event_types.contains(&"ToolExecEnd"));

    // Messages: user, assistant(tool_call), toolResult, assistant(text)
    assert_eq!(new_messages.len(), 4);
    assert_eq!(new_messages[0].role(), "user");
    assert_eq!(new_messages[1].role(), "assistant");
    assert_eq!(new_messages[2].role(), "toolResult");
    assert_eq!(new_messages[3].role(), "assistant");
}

#[tokio::test]
async fn test_abort_cancels_loop() {
    // Provider that returns text — but we cancel before it runs
    let provider = MockProvider::text("Should not appear");
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("Hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    // Cancel immediately
    cancel.cancel();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Should have user message but loop should exit early
    // The prompt is added before the loop checks cancellation
    assert!(new_messages.len() <= 2); // user + possibly error
}

#[tokio::test]
async fn test_continue_from_tool_result() {
    let provider = MockProvider::text("Done processing.");
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: vec![
            AgentMessage::Llm(Message::user("do something")),
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id: "tc-1".into(),
                tool_name: "test_tool".into(),
                content: vec![Content::Text {
                    text: "result".into(),
                }],
                is_error: false,
                timestamp: 0,
            }),
        ],
        tools: Vec::new(),
    };

    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop_continue(&mut context, &config, tx, cancel).await;

    assert!(!new_messages.is_empty());
    assert_eq!(new_messages[0].role(), "assistant");
}

#[tokio::test]
async fn test_tool_error_is_reported() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "failing_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("Tool failed, sorry.".into()),
    ]);

    struct FailingTool;

    #[async_trait::async_trait]
    impl AgentTool for FailingTool {
        fn name(&self) -> &str {
            "failing_tool"
        }
        fn label(&self) -> &str {
            "Failing Tool"
        }
        fn description(&self) -> &str {
            "Always fails"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Err(ToolError::Failed("Something went wrong".into()))
        }
    }

    let config = make_config(provider);
    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(FailingTool)],
    };

    let prompt = AgentMessage::Llm(Message::user("Use the tool"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);

    // Tool error should be reported
    let tool_end_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionEnd { is_error: true, .. }))
        .collect();
    assert_eq!(tool_end_events.len(), 1);

    // Should still get a final assistant response
    assert_eq!(new_messages.last().unwrap().role(), "assistant");
}

#[tokio::test]
async fn test_unknown_tool_reports_error() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "nonexistent".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("I couldn't find that tool.".into()),
    ]);

    let config = make_config(provider);
    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(), // No tools registered
    };

    let prompt = AgentMessage::Llm(Message::user("Use nonexistent tool"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let _new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);
    let tool_errors: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionEnd { is_error: true, .. }))
        .collect();
    assert_eq!(tool_errors.len(), 1);
}

// ---------------------------------------------------------------------------
// Parallel tool execution tests
// ---------------------------------------------------------------------------

/// A tool that records execution timestamps to verify parallelism.
struct TimedTool {
    name: String,
    delay_ms: u64,
}

#[async_trait::async_trait]
impl AgentTool for TimedTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn label(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "Timed tool"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("done:{}", self.name),
            }],
            details: serde_json::Value::Null,
        })
    }
}

#[tokio::test]
async fn test_parallel_tool_execution_faster_than_sequential() {
    // 3 tools each taking 50ms. Sequential = 150ms+, Parallel = ~50ms.
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "tool_a".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_b".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_c".into(),
                arguments: serde_json::json!({}),
            },
        ]),
        MockResponse::Text("All done.".into()),
    ]);

    let mut config = make_config(provider);
    config.tool_execution = ToolExecutionStrategy::Parallel;

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![
            Box::new(TimedTool {
                name: "tool_a".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_b".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_c".into(),
                delay_ms: 50,
            }),
        ],
    };

    let prompt = AgentMessage::Llm(Message::user("Run all tools"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let start = std::time::Instant::now();
    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let elapsed = start.elapsed();

    let events = collect_events(rx);

    // All 3 tool results should be present
    let tool_results: Vec<_> = new_messages
        .iter()
        .filter(|m| m.role() == "toolResult")
        .collect();
    assert_eq!(tool_results.len(), 3);

    // Should complete in roughly 50-100ms, not 150ms+
    assert!(
        elapsed.as_millis() < 130,
        "Parallel execution took {}ms, expected <130ms",
        elapsed.as_millis()
    );

    // Should have 3 ToolExecutionStart and 3 ToolExecutionEnd events
    let starts = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionStart { .. }))
        .count();
    let ends = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionEnd { .. }))
        .count();
    assert_eq!(starts, 3);
    assert_eq!(ends, 3);
}

#[tokio::test]
async fn test_sequential_tool_execution_is_slower() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "tool_a".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_b".into(),
                arguments: serde_json::json!({}),
            },
        ]),
        MockResponse::Text("Done.".into()),
    ]);

    let mut config = make_config(provider);
    config.tool_execution = ToolExecutionStrategy::Sequential;

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![
            Box::new(TimedTool {
                name: "tool_a".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_b".into(),
                delay_ms: 50,
            }),
        ],
    };

    let prompt = AgentMessage::Llm(Message::user("Run tools"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let start = std::time::Instant::now();
    let _new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let elapsed = start.elapsed();

    // Sequential should take 100ms+ (2 × 50ms)
    assert!(
        elapsed.as_millis() >= 95,
        "Sequential execution took {}ms, expected >=95ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn test_batched_tool_execution() {
    // 4 tools, batch size 2: two batches of 2
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "tool_a".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_b".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_c".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_d".into(),
                arguments: serde_json::json!({}),
            },
        ]),
        MockResponse::Text("All done.".into()),
    ]);

    let mut config = make_config(provider);
    config.tool_execution = ToolExecutionStrategy::Batched { size: 2 };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![
            Box::new(TimedTool {
                name: "tool_a".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_b".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_c".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_d".into(),
                delay_ms: 50,
            }),
        ],
    };

    let prompt = AgentMessage::Llm(Message::user("Run all tools"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let start = std::time::Instant::now();
    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let elapsed = start.elapsed();

    let _events = collect_events(rx);

    // All 4 results present
    let tool_results: Vec<_> = new_messages
        .iter()
        .filter(|m| m.role() == "toolResult")
        .collect();
    assert_eq!(tool_results.len(), 4);

    // 2 batches × 50ms = ~100ms (not 200ms sequential, not 50ms full parallel)
    assert!(
        elapsed.as_millis() >= 90 && elapsed.as_millis() < 160,
        "Batched execution took {}ms, expected 90-160ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Streaming tool output (on_update callback) tests
// ---------------------------------------------------------------------------

/// A tool that emits progress updates via on_update callback.
struct ProgressTool;

#[async_trait::async_trait]
impl AgentTool for ProgressTool {
    fn name(&self) -> &str {
        "progress_tool"
    }
    fn label(&self) -> &str {
        "Progress"
    }
    fn description(&self) -> &str {
        "A tool that streams progress"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        for i in 1..=3 {
            if let Some(ref cb) = ctx.on_update {
                cb(ToolResult {
                    content: vec![Content::Text {
                        text: format!("step {}/3", i),
                    }],
                    details: serde_json::Value::Null,
                });
            }
        }
        Ok(ToolResult {
            content: vec![Content::Text {
                text: "done".into(),
            }],
            details: serde_json::Value::Null,
        })
    }
}

#[tokio::test]
async fn test_tool_execution_update_events_emitted() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("All done.".into()),
    ]);

    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);

    let updates: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolExecutionUpdate { partial_result, .. } => {
                if let Some(Content::Text { text }) = partial_result.content.first() {
                    Some(text.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    assert_eq!(updates, vec!["step 1/3", "step 2/3", "step 3/3"]);
}

// ---------------------------------------------------------------------------
// Retry with backoff tests
// ---------------------------------------------------------------------------

/// A provider that fails N times with a given error, then delegates to a MockProvider.
struct FailThenSucceedProvider {
    fail_count: std::sync::atomic::AtomicUsize,
    max_failures: usize,
    error: ProviderError,
    inner: MockProvider,
}

use bendengine::provider::ProviderError;
use bendengine::provider::StreamConfig;
use bendengine::provider::StreamEvent;
use bendengine::provider::StreamProvider;

#[async_trait::async_trait]
impl StreamProvider for FailThenSucceedProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<bendengine::Message, ProviderError> {
        let attempt = self
            .fail_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if attempt < self.max_failures {
            return Err(match &self.error {
                ProviderError::RateLimited { retry_after_ms } => ProviderError::RateLimited {
                    retry_after_ms: *retry_after_ms,
                },
                ProviderError::Network(msg) => ProviderError::Network(msg.clone()),
                ProviderError::Auth(msg) => ProviderError::Auth(msg.clone()),
                other => ProviderError::Other(other.to_string()),
            });
        }
        self.inner.stream(config, tx, cancel).await
    }
}

#[tokio::test]
async fn test_retry_on_rate_limit_succeeds() {
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 2,
            error: ProviderError::RateLimited {
                retry_after_ms: Some(10), // 10ms for fast tests
            },
            inner: MockProvider::text("Success after retries"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_strategy: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            backoff_multiplier: 2.0,
            max_delay_ms: 100,
        },
        before_turn: None,
        after_turn: None,
        on_error: None,
        input_filters: vec![],
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Should have succeeded after 2 failures + 1 success
    assert_eq!(new_messages.len(), 2); // user + assistant
    let events = collect_events(rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { .. })));

    // Verify the provider was called 3 times (2 failures + 1 success)
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );
}

#[tokio::test]
async fn test_retry_exhausted_returns_error() {
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 10, // more failures than retries
            error: ProviderError::Network("connection reset".into()),
            inner: MockProvider::text("never reached"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_strategy: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig {
            max_retries: 2,
            initial_delay_ms: 10,
            backoff_multiplier: 2.0,
            max_delay_ms: 100,
        },
        before_turn: None,
        after_turn: None,
        on_error: None,
        input_filters: vec![],
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Should have an error message (StopReason::Error)
    let last = new_messages.last().unwrap();
    if let AgentMessage::Llm(Message::Assistant {
        stop_reason,
        error_message,
        ..
    }) = last
    {
        assert_eq!(*stop_reason, StopReason::Error);
        assert!(error_message.as_ref().unwrap().contains("connection reset"));
    } else {
        panic!("Expected error assistant message");
    }

    // 1 initial + 2 retries = 3 attempts
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );
}

#[tokio::test]
async fn test_retry_on_auth_error_succeeds() {
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 1,
            error: ProviderError::Auth("invalid key".into()),
            inner: MockProvider::text("recovered"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_strategy: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10,
            backoff_multiplier: 1.0,
            max_delay_ms: 50,
        },
        before_turn: None,
        after_turn: None,
        on_error: None,
        input_filters: vec![],
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Should have retried after the auth error and succeeded on 2nd attempt
    // fail_count is incremented on every call (fail + success), so 1 fail + 1 success = 2
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    // Context should have the recovered response
    assert!(!context.messages.is_empty());
}

#[tokio::test]
async fn test_retry_none_disables_retries() {
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 1,
            error: ProviderError::RateLimited {
                retry_after_ms: None,
            },
            inner: MockProvider::text("never reached"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_strategy: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig::none(), // disabled
        before_turn: None,
        after_turn: None,
        on_error: None,
        input_filters: vec![],
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Only 1 attempt — no retries
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

// ---------------------------------------------------------------------------
// Event streaming bug fix test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_message_update_events_emitted_during_streaming() {
    // This test verifies the fix for: text deltas not emitted because
    // partial_message was None when deltas arrived (MessageStart was only
    // emitted on Done, after all deltas had already been processed).
    let provider = MockProvider::text("Hello, world!");
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);

    // Collect MessageUpdate text deltas
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::MessageUpdate {
                delta: StreamDelta::Text { delta },
                ..
            } => Some(delta.clone()),
            _ => None,
        })
        .collect();

    // Should have at least one text delta with "Hello, world!"
    assert!(
        !deltas.is_empty(),
        "Expected MessageUpdate events with text deltas, got none"
    );
    let full_text: String = deltas.into_iter().collect();
    assert_eq!(full_text, "Hello, world!");

    // Verify event ordering: MessageStart before MessageUpdate before MessageEnd
    let event_types: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::MessageStart { .. } => Some("Start"),
            AgentEvent::MessageUpdate { .. } => Some("Update"),
            AgentEvent::MessageEnd { .. } => Some("End"),
            _ => None,
        })
        .collect();

    // Should be: Start (user), End (user), Start (assistant), Update(s), End (assistant)
    // Find the assistant sequence
    let assistant_start = event_types.iter().rposition(|&e| e == "Start").unwrap();
    let assistant_end = event_types.iter().rposition(|&e| e == "End").unwrap();

    // All Updates should be between the last Start and last End
    for (i, &et) in event_types.iter().enumerate() {
        if et == "Update" {
            assert!(
                i > assistant_start && i < assistant_end,
                "MessageUpdate at index {} should be between MessageStart ({}) and MessageEnd ({})",
                i,
                assistant_start,
                assistant_end
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle callback tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_before_turn_can_abort() {
    // Provider with 5 text responses, but before_turn aborts after 2 turns.
    // We need tool calls to keep the loop going for multiple turns.
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        // These should never be reached
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("Final".into()),
    ]);

    let turn_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let turn_count_clone = turn_count.clone();

    let mut config = make_config(provider);
    config.before_turn = Some(std::sync::Arc::new(move |_msgs, _turn| {
        let count = turn_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        count < 2 // Allow turns 0 and 1, abort on turn 2
    }));

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // before_turn was called 3 times (allowed 0, allowed 1, rejected 2)
    assert_eq!(turn_count.load(std::sync::atomic::Ordering::SeqCst), 3);

    // Only 2 assistant messages should be produced
    let assistant_count = new_messages
        .iter()
        .filter(|m| m.role() == "assistant")
        .count();
    assert_eq!(assistant_count, 2);
}

#[tokio::test]
async fn test_after_turn_receives_messages() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("Done.".into()),
    ]);

    let message_counts: std::sync::Arc<std::sync::Mutex<Vec<usize>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let counts_clone = message_counts.clone();

    let mut config = make_config(provider);
    config.after_turn = Some(std::sync::Arc::new(move |msgs, _usage| {
        counts_clone.lock().unwrap().push(msgs.len());
    }));

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let counts = message_counts.lock().unwrap();
    // after_turn called twice (one per LLM response)
    assert_eq!(counts.len(), 2);
    // Message count should increase between calls
    assert!(counts[1] > counts[0], "counts: {:?}", *counts);
}

#[tokio::test]
async fn test_on_error_fires_on_provider_error() {
    let provider = FailThenSucceedProvider {
        fail_count: std::sync::atomic::AtomicUsize::new(0),
        max_failures: 10, // more failures than retries
        error: ProviderError::Network("connection reset".into()),
        inner: MockProvider::text("never reached"),
    };

    let error_msgs: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let error_msgs_clone = error_msgs.clone();

    let config = AgentLoopConfig {
        provider: std::sync::Arc::new(provider),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_strategy: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig::none(),
        before_turn: None,
        after_turn: None,
        on_error: Some(std::sync::Arc::new(move |err| {
            error_msgs_clone.lock().unwrap().push(err.to_string());
        })),
        input_filters: vec![],
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let errors = error_msgs.lock().unwrap();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("connection reset"), "got: {}", errors[0]);
}

#[tokio::test]
async fn test_callbacks_are_optional() {
    // Verify the loop works fine with all callbacks set to None (same as before)
    let provider = MockProvider::text("Hello!");
    let config = make_config(provider);
    // make_config already sets all callbacks to None

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("Hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    assert_eq!(new_messages.len(), 2);
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { .. })));
}

// ---------------------------------------------------------------------------
// ProgressMessage tests (Addition 1)
// ---------------------------------------------------------------------------

/// A tool that calls on_progress to emit user-facing progress messages.
struct ProgressMessageTool;

#[async_trait::async_trait]
impl AgentTool for ProgressMessageTool {
    fn name(&self) -> &str {
        "progress_msg_tool"
    }
    fn label(&self) -> &str {
        "ProgressMsg"
    }
    fn description(&self) -> &str {
        "Emits progress messages"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        if let Some(ref progress) = ctx.on_progress {
            progress("Working...".into());
        }
        Ok(ToolResult {
            content: vec![Content::Text {
                text: "done".into(),
            }],
            details: serde_json::Value::Null,
        })
    }
}

#[tokio::test]
async fn test_progress_message_event_emitted() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_msg_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("ok".into()),
    ]);
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressMessageTool)],
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    let progress_msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ProgressMessage {
                tool_call_id,
                tool_name,
                text,
            } => Some((tool_call_id.clone(), tool_name.clone(), text.clone())),
            _ => None,
        })
        .collect();

    assert_eq!(progress_msgs.len(), 1);
    assert_eq!(progress_msgs[0].1, "progress_msg_tool");
    assert_eq!(progress_msgs[0].2, "Working...");
}

/// A tool that does NOT call on_progress — should cause no panics, no events.
struct SilentTool;

#[async_trait::async_trait]
impl AgentTool for SilentTool {
    fn name(&self) -> &str {
        "silent_tool"
    }
    fn label(&self) -> &str {
        "Silent"
    }
    fn description(&self) -> &str {
        "Does not call progress"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        // Intentionally ignores on_progress
        Ok(ToolResult {
            content: vec![Content::Text {
                text: "quiet".into(),
            }],
            details: serde_json::Value::Null,
        })
    }
}

#[tokio::test]
async fn test_tool_ignoring_progress_no_panic() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "silent_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("ok".into()),
    ]);
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(SilentTool)],
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    // No ProgressMessage events
    let progress_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ProgressMessage { .. }))
        .count();
    assert_eq!(progress_count, 0);
}

/// Two parallel tools both emit progress — events are distinguishable by tool_call_id.
struct NamedProgressTool {
    tool_name: String,
}

#[async_trait::async_trait]
impl AgentTool for NamedProgressTool {
    fn name(&self) -> &str {
        &self.tool_name
    }
    fn label(&self) -> &str {
        &self.tool_name
    }
    fn description(&self) -> &str {
        "Named progress tool"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        if let Some(ref progress) = ctx.on_progress {
            progress(format!("progress from {}", self.tool_name));
        }
        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("done:{}", self.tool_name),
            }],
            details: serde_json::Value::Null,
        })
    }
}

#[tokio::test]
async fn test_parallel_tools_progress_distinguishable() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "pa".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "pb".into(),
                arguments: serde_json::json!({}),
            },
        ]),
        MockResponse::Text("done".into()),
    ]);
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![
            Box::new(NamedProgressTool {
                tool_name: "pa".into(),
            }),
            Box::new(NamedProgressTool {
                tool_name: "pb".into(),
            }),
        ],
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    let progress_msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ProgressMessage {
                tool_name, text, ..
            } => Some((tool_name.clone(), text.clone())),
            _ => None,
        })
        .collect();

    assert_eq!(progress_msgs.len(), 2);
    let names: Vec<&str> = progress_msgs.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"pa"));
    assert!(names.contains(&"pb"));
}

#[tokio::test]
async fn test_on_update_still_works_after_refactor() {
    // Existing ProgressTool uses on_update (not on_progress) — ensure it still works.
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("ok".into()),
    ]);
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    let updates: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolExecutionUpdate { partial_result, .. } => {
                if let Some(Content::Text { text }) = partial_result.content.first() {
                    Some(text.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    assert_eq!(updates, vec!["step 1/3", "step 2/3", "step 3/3"]);
}

// ---------------------------------------------------------------------------
// InputFilter tests (Addition 2)
// ---------------------------------------------------------------------------

use std::sync::Arc;

struct PassFilter;
impl InputFilter for PassFilter {
    fn filter(&self, _text: &str) -> FilterResult {
        FilterResult::Pass
    }
}

struct WarnFilter {
    warning: String,
}
impl InputFilter for WarnFilter {
    fn filter(&self, _text: &str) -> FilterResult {
        FilterResult::Warn(self.warning.clone())
    }
}

struct RejectFilter {
    reason: String,
}
impl InputFilter for RejectFilter {
    fn filter(&self, _text: &str) -> FilterResult {
        FilterResult::Reject(self.reason.clone())
    }
}

#[tokio::test]
async fn test_filter_pass_message_goes_through() {
    let provider = MockProvider::text("Hello!");
    let mut config = make_config(provider);
    config.input_filters = vec![Arc::new(PassFilter)];

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("Hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    // Message went through normally
    assert_eq!(new_messages.len(), 2); // user + assistant
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { .. })));
}

#[tokio::test]
async fn test_filter_warn_injects_warning_message() {
    let provider = MockProvider::text("Got it.");
    let mut config = make_config(provider);
    config.input_filters = vec![Arc::new(WarnFilter {
        warning: "danger".into(),
    })];

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("Hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // user (with appended warning) + assistant = 2
    assert_eq!(new_messages.len(), 2);
    // The warning should be appended to the user message's content
    if let AgentMessage::Llm(Message::User { content, .. }) = &new_messages[0] {
        assert_eq!(content.len(), 2, "expected original text + warning");
        let warning = match &content[1] {
            Content::Text { text } => text.as_str(),
            _ => panic!("expected text"),
        };
        assert!(warning.contains("[Warning: danger]"), "got: {}", warning);
    } else {
        panic!("Expected user message at index 0");
    }
}

#[tokio::test]
async fn test_filter_reject_returns_empty() {
    let provider = MockProvider::text("Should not reach");
    let mut config = make_config(provider);
    config.input_filters = vec![Arc::new(RejectFilter {
        reason: "blocked".into(),
    })];

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("Bad input"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    // Rejected — empty messages returned
    assert!(new_messages.is_empty());
    // Context should NOT contain the rejected prompt
    assert!(
        context.messages.is_empty(),
        "Rejected prompts should not leak into context, got {} messages",
        context.messages.len()
    );
    // InputRejected event should carry the reason
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::InputRejected { reason } if reason == "blocked")));
    // AgentStart + InputRejected + AgentEnd
    assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentStart)));
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { messages } if messages.is_empty())));
}

#[tokio::test]
async fn test_filter_chain_first_reject_wins() {
    let provider = MockProvider::text("Should not reach");
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    struct CountingRejectFilter {
        counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    impl InputFilter for CountingRejectFilter {
        fn filter(&self, _text: &str) -> FilterResult {
            self.counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            FilterResult::Reject("first rejects".into())
        }
    }

    struct NeverCalledFilter {
        counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    impl InputFilter for NeverCalledFilter {
        fn filter(&self, _text: &str) -> FilterResult {
            self.counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            FilterResult::Pass
        }
    }

    let count2 = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut config = make_config(provider);
    config.input_filters = vec![
        Arc::new(CountingRejectFilter {
            counter: call_count.clone(),
        }),
        Arc::new(NeverCalledFilter {
            counter: count2.clone(),
        }),
    ];

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("Bad"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    assert!(new_messages.is_empty());
    // First filter was called
    assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    // Second filter was NOT called (first reject short-circuits)
    assert_eq!(count2.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_filter_multiple_warns_accumulate() {
    let provider = MockProvider::text("Got warnings.");
    let mut config = make_config(provider);
    config.input_filters = vec![
        Arc::new(WarnFilter {
            warning: "warn1".into(),
        }),
        Arc::new(WarnFilter {
            warning: "warn2".into(),
        }),
    ];

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("Hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // user (with appended warnings) + assistant = 2
    assert_eq!(new_messages.len(), 2);
    if let AgentMessage::Llm(Message::User { content, .. }) = &new_messages[0] {
        // Original text + appended warning block
        assert!(content.len() >= 2, "expected original text + warning");
        let warning = match content.last().unwrap() {
            Content::Text { text } => text.as_str(),
            _ => panic!("expected text"),
        };
        assert!(warning.contains("[Warning: warn1]"), "got: {}", warning);
        assert!(warning.contains("[Warning: warn2]"), "got: {}", warning);
    } else {
        panic!("Expected user message");
    }
}

#[tokio::test]
async fn test_filter_non_text_content_only_text_extracted() {
    // User message with Image content — filter should receive only text portions
    let provider = MockProvider::text("Ok");

    let call_text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let call_text_clone = call_text.clone();

    struct CapturingFilter {
        captured: std::sync::Arc<std::sync::Mutex<String>>,
    }
    impl InputFilter for CapturingFilter {
        fn filter(&self, text: &str) -> FilterResult {
            *self.captured.lock().unwrap() = text.to_string();
            FilterResult::Pass
        }
    }

    let mut config = make_config(provider);
    config.input_filters = vec![Arc::new(CapturingFilter {
        captured: call_text_clone,
    })];

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::User {
        content: vec![
            Content::Text {
                text: "Check this image".into(),
            },
            Content::Image {
                data: "base64data".into(),
                mime_type: "image/png".into(),
            },
        ],
        timestamp: 0,
    });
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let captured = call_text.lock().unwrap();
    // Filter should have received only the text portion
    assert_eq!(*captured, "Check this image");
}

// ---------------------------------------------------------------------------
// CompactionStrategy tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_default_compaction_matches_compact_messages() {
    use bendengine::context::compact_messages;
    use bendengine::context::ContextConfig;
    use bendengine::context::DefaultCompaction;
    use bendengine::CompactionStrategy;

    let mut messages = Vec::new();
    for i in 0..100 {
        messages.push(AgentMessage::Llm(Message::user(format!(
            "Message {} {}",
            i,
            "x".repeat(200)
        ))));
    }

    let config = ContextConfig {
        max_context_tokens: 500,
        system_prompt_tokens: 100,
        keep_recent: 5,
        keep_first: 2,
        tool_output_max_lines: 20,
    };

    let result_direct = compact_messages(messages.clone(), &config);
    let result_trait = DefaultCompaction.compact(messages, &config);

    assert_eq!(result_direct.messages.len(), result_trait.messages.len());
    assert!(
        result_direct.messages.len() < 100,
        "compaction should have reduced messages"
    );
    assert!(
        result_direct.messages.len() >= 2,
        "should keep at least keep_first messages"
    );
}

#[tokio::test]
async fn test_custom_compaction_strategy_is_called() {
    use bendengine::context::ContextConfig;
    use bendengine::CompactionResult;
    use bendengine::CompactionStats;
    use bendengine::CompactionStrategy;

    struct MarkerCompaction;

    impl CompactionStrategy for MarkerCompaction {
        fn compact(
            &self,
            messages: Vec<AgentMessage>,
            _config: &ContextConfig,
        ) -> CompactionResult {
            let mut result = vec![AgentMessage::Llm(Message::user("[compacted]"))];
            if let Some(last) = messages.last() {
                result.push(last.clone());
            }
            CompactionResult {
                messages: result,
                stats: CompactionStats::default(),
            }
        }
    }

    // Provider returns a simple text response
    let provider = MockProvider::text("Got it.");

    let config = AgentLoopConfig {
        provider: std::sync::Arc::new(provider),
        model: "test".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: Some(ContextConfig {
            max_context_tokens: 10, // Tiny budget to force compaction
            system_prompt_tokens: 0,
            keep_recent: 1,
            keep_first: 1,
            tool_output_max_lines: 10,
        }),
        compaction_strategy: Some(std::sync::Arc::new(MarkerCompaction)),
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig::none(),
        before_turn: None,
        after_turn: None,
        on_error: None,
        input_filters: vec![],
    };

    let prompt = AgentMessage::Llm(Message::user("Hello"));
    let mut context = AgentContext {
        system_prompt: String::new(),
        messages: vec![],
        tools: vec![],
    };

    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // The custom strategy should have inserted "[compacted]" as the first message
    assert!(
        context.messages.iter().any(|m| {
            if let AgentMessage::Llm(Message::User { content, .. }) = m {
                content
                    .iter()
                    .any(|c| matches!(c, Content::Text { text } if text == "[compacted]"))
            } else {
                false
            }
        }),
        "Custom compaction marker not found in context: {:?}",
        context
            .messages
            .iter()
            .filter_map(|m| {
                if let AgentMessage::Llm(Message::User { content, .. }) = m {
                    Some(content)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_none_compaction_strategy_uses_default() {
    use bendengine::context::ContextConfig;

    // Provider returns a simple text response
    let provider = MockProvider::text("Got it.");

    let config = AgentLoopConfig {
        provider: std::sync::Arc::new(provider),
        model: "test".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: Some(ContextConfig {
            max_context_tokens: 10, // Tiny budget to force compaction
            system_prompt_tokens: 0,
            keep_recent: 1,
            keep_first: 1,
            tool_output_max_lines: 10,
        }),
        compaction_strategy: None, // Should fall back to DefaultCompaction
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig::none(),
        before_turn: None,
        after_turn: None,
        on_error: None,
        input_filters: vec![],
    };

    let prompt = AgentMessage::Llm(Message::user("Hello"));
    let mut context = AgentContext {
        system_prompt: String::new(),
        messages: vec![],
        tools: vec![],
    };

    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    // Should not panic — DefaultCompaction handles everything
    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Agent should have produced at least the user message + assistant response
    assert!(
        !new_messages.is_empty(),
        "Agent should have produced messages"
    );
}

// ---------------------------------------------------------------------------
// Context compaction event tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_compaction_events_emitted_when_context_exceeds_budget() {
    use bendengine::context::ContextConfig;

    let provider = MockProvider::text("Got it.");

    let mut prior_messages = Vec::new();
    for i in 0..20 {
        prior_messages.push(AgentMessage::Llm(Message::user(format!(
            "msg {} {}",
            i,
            "x".repeat(500)
        ))));
        prior_messages.push(AgentMessage::Llm(Message::Assistant {
            content: vec![Content::Text {
                text: format!("reply {} {}", i, "y".repeat(500)),
            }],
            stop_reason: StopReason::Stop,
            model: "mock".into(),
            provider: "mock".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }));
    }

    let config = AgentLoopConfig {
        provider: std::sync::Arc::new(provider),
        model: "test".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: Some(ContextConfig {
            max_context_tokens: 500,
            system_prompt_tokens: 0,
            keep_recent: 4,
            keep_first: 2,
            tool_output_max_lines: 20,
        }),
        compaction_strategy: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig::none(),
        before_turn: None,
        after_turn: None,
        on_error: None,
        input_filters: vec![],
    };

    let prompt = AgentMessage::Llm(Message::user("hello"));
    let mut context = AgentContext {
        system_prompt: String::new(),
        messages: prior_messages,
        tools: vec![],
    };

    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    let starts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ContextCompactionStart {
                message_count,
                estimated_tokens,
                ..
            } => Some((*message_count, *estimated_tokens)),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 1);
    assert!(starts[0].0 > 20);
    assert!(starts[0].1 > 500);

    let ends: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ContextCompactionEnd { stats, .. } => Some(stats.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ends.len(), 1);
    let stats = &ends[0];
    assert!(stats.level > 0);
    assert!(stats.after_message_count < stats.before_message_count);
    assert!(stats.after_estimated_tokens < stats.before_estimated_tokens);
}

#[tokio::test]
async fn test_compaction_events_level_zero_when_within_budget() {
    use bendengine::context::ContextConfig;

    let provider = MockProvider::text("ok");

    let config = AgentLoopConfig {
        provider: std::sync::Arc::new(provider),
        model: "test".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        temperature: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: Some(ContextConfig {
            max_context_tokens: 1_000_000,
            system_prompt_tokens: 0,
            keep_recent: 10,
            keep_first: 2,
            tool_output_max_lines: 50,
        }),
        compaction_strategy: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_config: bendengine::RetryConfig::none(),
        before_turn: None,
        after_turn: None,
        on_error: None,
        input_filters: vec![],
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let mut context = AgentContext {
        system_prompt: String::new(),
        messages: vec![],
        tools: vec![],
    };

    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    let ends: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ContextCompactionEnd { stats, .. } => Some(stats.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0].level, 0);
    assert_eq!(ends[0].before_message_count, ends[0].after_message_count);
}

#[tokio::test]
async fn test_compaction_not_emitted_without_context_config() {
    let provider = MockProvider::text("ok");
    let config = make_config(provider);

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let mut context = AgentContext {
        system_prompt: String::new(),
        messages: vec![],
        tools: vec![],
    };

    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    let compaction_events = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                AgentEvent::ContextCompactionStart { .. } | AgentEvent::ContextCompactionEnd { .. }
            )
        })
        .count();
    assert_eq!(compaction_events, 0);
}
