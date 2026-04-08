//! Tests for SubAgentTool using MockProvider.

use std::sync::Arc;

use bendengine::agent_loop::agent_loop;
use bendengine::agent_loop::AgentLoopConfig;
use bendengine::provider::mock::*;
use bendengine::provider::MockProvider;
use bendengine::sub_agent::SubAgentTool;
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

// ---------------------------------------------------------------------------
// Basic sub-agent execution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sub_agent_basic() {
    // The sub-agent's mock provider returns a simple text response
    let sub_provider = Arc::new(MockProvider::text("Research result: Rust is great"));

    let sub_agent = SubAgentTool::new("researcher", sub_provider)
        .with_description("Researches topics")
        .with_system_prompt("You are a research assistant.")
        .with_model("mock")
        .with_api_key("test");

    // Execute the sub-agent tool directly
    let params = serde_json::json!({"task": "Tell me about Rust"});

    let result = sub_agent
        .execute(params, ToolContext {
            tool_call_id: "tc-1".into(),
            tool_name: "researcher".into(),
            cancel: CancellationToken::new(),
            on_update: None,
            on_progress: None,
        })
        .await
        .expect("sub-agent should succeed");

    // Should contain the sub-agent's response text
    let text = match &result.content[0] {
        Content::Text { text } => text.as_str(),
        _ => panic!("Expected text content"),
    };
    assert_eq!(text, "Research result: Rust is great");

    // Details should include sub-agent metadata
    assert_eq!(result.details["sub_agent"], "researcher");
}

// ---------------------------------------------------------------------------
// Sub-agent with its own tools
// ---------------------------------------------------------------------------

struct EchoTool;

#[async_trait::async_trait]
impl AgentTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn label(&self) -> &str {
        "Echo"
    }
    fn description(&self) -> &str {
        "Echoes input"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {"type": "string"}
            }
        })
    }
    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let text = params["text"].as_str().unwrap_or("(empty)");
        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("echoed: {}", text),
            }],
            details: serde_json::Value::Null,
        })
    }
}

#[tokio::test]
async fn test_sub_agent_with_tools() {
    // Sub-agent first calls the echo tool, then responds with text
    let sub_provider = Arc::new(MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "echo".into(),
            arguments: serde_json::json!({"text": "hello"}),
        }]),
        MockResponse::Text("The echo returned: echoed: hello".into()),
    ]));

    let echo_tool: Arc<dyn AgentTool> = Arc::new(EchoTool);

    let sub_agent = SubAgentTool::new("echo_agent", sub_provider)
        .with_description("Agent that echoes")
        .with_system_prompt("Use the echo tool.")
        .with_model("mock")
        .with_api_key("test")
        .with_tools(vec![echo_tool]);

    let params = serde_json::json!({"task": "Echo hello"});

    let result = sub_agent
        .execute(params, ToolContext {
            tool_call_id: "tc-1".into(),
            tool_name: "echo_agent".into(),
            cancel: CancellationToken::new(),
            on_update: None,
            on_progress: None,
        })
        .await
        .expect("sub-agent should succeed");

    let text = match &result.content[0] {
        Content::Text { text } => text.as_str(),
        _ => panic!("Expected text content"),
    };
    assert_eq!(text, "The echo returned: echoed: hello");
}

// ---------------------------------------------------------------------------
// Cancellation propagation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sub_agent_cancellation() {
    // Sub-agent provider returns text, but we cancel before execution
    let sub_provider = Arc::new(MockProvider::text("Should not appear"));

    let sub_agent = SubAgentTool::new("cancelled_agent", sub_provider)
        .with_model("mock")
        .with_api_key("test");

    let cancel = CancellationToken::new();
    cancel.cancel(); // Cancel immediately

    let params = serde_json::json!({"task": "Do something"});

    let result = sub_agent
        .execute(params, ToolContext {
            tool_call_id: "tc-1".into(),
            tool_name: "cancelled_agent".into(),
            cancel,
            on_update: None,
            on_progress: None,
        })
        .await
        .expect("should return a result even when cancelled");

    // When cancelled before the loop runs, we get the fallback message
    let text = match &result.content[0] {
        Content::Text { text } => text.as_str(),
        _ => panic!("Expected text content"),
    };
    // The loop exits early on cancellation, so the mock response should not appear
    assert_ne!(
        text, "Should not appear",
        "Sub-agent ran despite cancellation"
    );
}

// ---------------------------------------------------------------------------
// Max turns limit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sub_agent_max_turns() {
    // Sub-agent keeps calling tools indefinitely — max_turns should stop it.
    // With max_turns=1, the sub-agent gets 1 LLM call.
    // Response 1: tool call → executes tool → hits turn limit → returns limit message
    // The sub-agent won't get a second LLM call to produce text.
    let sub_provider = Arc::new(MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "echo".into(),
            arguments: serde_json::json!({"text": "loop"}),
        }]),
        // This response won't be reached due to turn limit
        MockResponse::Text("Should not reach".into()),
    ]));

    let echo_tool: Arc<dyn AgentTool> = Arc::new(EchoTool);

    let sub_agent = SubAgentTool::new("limited_agent", sub_provider)
        .with_model("mock")
        .with_api_key("test")
        .with_tools(vec![echo_tool])
        .with_max_turns(1); // Only 1 turn allowed

    let params = serde_json::json!({"task": "Keep going"});

    let result = sub_agent
        .execute(params, ToolContext {
            tool_call_id: "tc-1".into(),
            tool_name: "limited_agent".into(),
            cancel: CancellationToken::new(),
            on_update: None,
            on_progress: None,
        })
        .await
        .expect("sub-agent should succeed");

    // The sub-agent was stopped by turn limit — it won't have the second text response
    let text = match &result.content[0] {
        Content::Text { text } => text.as_str(),
        _ => panic!("Expected text content"),
    };
    // Should NOT contain the text from the second response
    assert_ne!(text, "Should not reach");
}

// ---------------------------------------------------------------------------
// Parallel sub-agent execution (via parent agent loop)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sub_agent_parallel() {
    // Two sub-agents that each take ~50ms, run in parallel via the parent loop.
    // The parent's mock emits both sub-agent tool calls, then a final text.

    struct SlowProvider {
        delay_ms: u64,
        text: String,
    }

    #[async_trait::async_trait]
    impl bendengine::provider::StreamProvider for SlowProvider {
        async fn stream(
            &self,
            _config: bendengine::provider::StreamConfig,
            tx: tokio::sync::mpsc::UnboundedSender<bendengine::provider::StreamEvent>,
            cancel: tokio_util::sync::CancellationToken,
        ) -> Result<Message, bendengine::provider::ProviderError> {
            if cancel.is_cancelled() {
                return Err(bendengine::provider::ProviderError::Cancelled);
            }
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;

            let _ = tx.send(bendengine::provider::StreamEvent::Start);
            let _ = tx.send(bendengine::provider::StreamEvent::TextDelta {
                content_index: 0,
                delta: self.text.clone(),
            });
            let msg = Message::Assistant {
                content: vec![Content::Text {
                    text: self.text.clone(),
                }],
                stop_reason: StopReason::Stop,
                model: "slow".into(),
                provider: "slow".into(),
                usage: Usage::default(),
                timestamp: bendengine::now_ms(),
                error_message: None,
            };
            let _ = tx.send(bendengine::provider::StreamEvent::Done {
                message: msg.clone(),
            });
            Ok(msg)
        }
    }

    let sub_a = SubAgentTool::new(
        "agent_a",
        Arc::new(SlowProvider {
            delay_ms: 50,
            text: "Result A".into(),
        }),
    )
    .with_model("slow")
    .with_api_key("test");

    let sub_b = SubAgentTool::new(
        "agent_b",
        Arc::new(SlowProvider {
            delay_ms: 50,
            text: "Result B".into(),
        }),
    )
    .with_model("slow")
    .with_api_key("test");

    // Parent provider: first call triggers both sub-agents, second returns final text
    let parent_provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "agent_a".into(),
                arguments: serde_json::json!({"task": "Do A"}),
            },
            MockToolCall {
                name: "agent_b".into(),
                arguments: serde_json::json!({"task": "Do B"}),
            },
        ]),
        MockResponse::Text("Both sub-agents completed.".into()),
    ]);

    let config = make_config(parent_provider);

    let mut context = AgentContext {
        system_prompt: "You are a coordinator.".into(),
        messages: Vec::new(),
        tools: vec![Box::new(sub_a), Box::new(sub_b)],
    };

    let prompt = AgentMessage::Llm(Message::user("Run both agents"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let start = std::time::Instant::now();
    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let elapsed = start.elapsed();

    let _events = collect_events(rx);

    // Both tool results should be present
    let tool_results: Vec<_> = new_messages
        .iter()
        .filter(|m| m.role() == "toolResult")
        .collect();
    assert_eq!(tool_results.len(), 2);

    // Should complete in roughly 50-100ms (parallel), not 100ms+ (sequential)
    assert!(
        elapsed.as_millis() < 130,
        "Parallel sub-agents took {}ms, expected <130ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Event forwarding via on_update
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sub_agent_event_forwarding() {
    let sub_provider = Arc::new(MockProvider::text("Sub-agent done"));

    let sub_agent = SubAgentTool::new("streaming_agent", sub_provider)
        .with_model("mock")
        .with_api_key("test");

    let params = serde_json::json!({"task": "Do work"});

    // Collect on_update calls
    let updates: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let updates_clone = updates.clone();
    let on_update: ToolUpdateFn = Arc::new(move |result: ToolResult| {
        if let Some(Content::Text { text }) = result.content.first() {
            updates_clone.lock().unwrap().push(text.clone());
        }
    });

    let result = sub_agent
        .execute(params, ToolContext {
            tool_call_id: "tc-1".into(),
            tool_name: "streaming_agent".into(),
            cancel: CancellationToken::new(),
            on_update: Some(on_update),
            on_progress: None,
        })
        .await
        .expect("sub-agent should succeed");

    // Final result should contain the sub-agent's text
    let text = match &result.content[0] {
        Content::Text { text } => text.as_str(),
        _ => panic!("Expected text content"),
    };
    assert_eq!(text, "Sub-agent done");

    // on_update should have received streaming deltas
    let collected = updates.lock().unwrap();
    assert!(
        !collected.is_empty(),
        "Expected on_update to receive streaming events"
    );
    // Should contain the text delta from the mock provider
    assert!(
        collected.iter().any(|t| t.contains("Sub-agent done")),
        "Expected text delta in updates, got: {:?}",
        *collected
    );
}

// ---------------------------------------------------------------------------
// Invalid parameters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sub_agent_missing_task_parameter() {
    let sub_provider = Arc::new(MockProvider::text("Should not run"));

    let sub_agent = SubAgentTool::new("test_agent", sub_provider)
        .with_model("mock")
        .with_api_key("test");

    let params = serde_json::json!({}); // Missing "task"

    let result = sub_agent
        .execute(params, ToolContext {
            tool_call_id: "tc-1".into(),
            tool_name: "test_agent".into(),
            cancel: CancellationToken::new(),
            on_update: None,
            on_progress: None,
        })
        .await;
    assert!(result.is_err());

    match result.unwrap_err() {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("task")),
        other => panic!("Expected InvalidArgs, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Integration: sub-agent tool in a parent agent loop
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sub_agent_in_parent_loop() {
    // Parent calls sub-agent, sub-agent returns text, parent summarizes
    let sub_provider = Arc::new(MockProvider::text("42 is the answer"));

    let sub_agent = SubAgentTool::new("calculator", sub_provider)
        .with_description("Calculates things")
        .with_model("mock")
        .with_api_key("test");

    let parent_provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "calculator".into(),
            arguments: serde_json::json!({"task": "What is 6*7?"}),
        }]),
        MockResponse::Text("The calculator says: 42 is the answer".into()),
    ]);

    let config = make_config(parent_provider);

    let mut context = AgentContext {
        system_prompt: "You are a coordinator.".into(),
        messages: Vec::new(),
        tools: vec![Box::new(sub_agent)],
    };

    let prompt = AgentMessage::Llm(Message::user("What is 6*7?"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);

    // Should have: user, assistant(tool_call), toolResult, assistant(text)
    assert_eq!(new_messages.len(), 4);
    assert_eq!(new_messages[0].role(), "user");
    assert_eq!(new_messages[1].role(), "assistant");
    assert_eq!(new_messages[2].role(), "toolResult");
    assert_eq!(new_messages[3].role(), "assistant");

    // Tool result should contain sub-agent's output
    if let AgentMessage::Llm(Message::ToolResult { content, .. }) = &new_messages[2] {
        let text = match &content[0] {
            Content::Text { text } => text.as_str(),
            _ => panic!("Expected text content"),
        };
        assert_eq!(text, "42 is the answer");
    } else {
        panic!("Expected tool result message");
    }

    // Should have tool execution events
    let has_tool_start = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolExecutionStart { tool_name, .. } if tool_name == "calculator"));
    let has_tool_end = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolExecutionEnd { tool_name, .. } if tool_name == "calculator"));
    assert!(has_tool_start);
    assert!(has_tool_end);
}
