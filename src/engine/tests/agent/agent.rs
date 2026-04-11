//! Tests for the Agent struct (stateful wrapper).

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use bendengine::agent::Agent;
use bendengine::provider::mock::*;
use bendengine::provider::MockProvider;
use bendengine::*;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_agent_simple_prompt() {
    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("You are helpful.")
        .with_model("mock")
        .with_api_key("test");

    let mut rx = agent.prompt("Hi there").await;

    // Drain events
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }

    agent.finish().await;
    assert!(!events.is_empty());
    assert_eq!(agent.messages().len(), 2); // user + assistant
}

#[tokio::test]
async fn test_agent_reset() {
    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    let mut rx = agent.prompt("Hi").await;
    while rx.recv().await.is_some() {}
    agent.finish().await;
    assert!(!agent.messages().is_empty());

    agent.reset().await;
    assert!(agent.messages().is_empty());
    assert!(!agent.is_streaming());
}

#[tokio::test]
async fn test_agent_with_tools() {
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
            serde_json::json!({"type": "object", "properties": {"text": {"type": "string"}}})
        }
        async fn execute(
            &self,
            params: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<ToolResult, ToolError> {
            let text = params["text"].as_str().unwrap_or("").to_string();
            Ok(ToolResult {
                content: vec![Content::Text { text }],
                details: serde_json::Value::Null,
            })
        }
    }

    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "echo".into(),
            arguments: serde_json::json!({"text": "hello"}),
        }]),
        MockResponse::Text("Echoed: hello".into()),
    ]);

    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test")
        .with_tools(vec![Box::new(EchoTool)]);

    let mut rx = agent.prompt("Echo hello").await;
    while rx.recv().await.is_some() {}
    agent.finish().await;

    // user + assistant(tool_call) + toolResult + assistant(text)
    assert_eq!(agent.messages().len(), 4);
}

#[tokio::test]
async fn test_agent_builder_pattern() {
    let provider = MockProvider::text("ok");
    let agent = Agent::new(provider)
        .with_system_prompt("sys")
        .with_model("test-model")
        .with_api_key("key123")
        .with_thinking(ThinkingLevel::Medium)
        .with_max_tokens(4096);

    assert_eq!(agent.system_prompt, "sys");
    assert_eq!(agent.model, "test-model");
    assert_eq!(agent.api_key, "key123");
    assert_eq!(agent.thinking_level, ThinkingLevel::Medium);
    assert_eq!(agent.max_tokens, Some(4096));
}

// ---------------------------------------------------------------------------
// State persistence tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_with_messages_builder() {
    let saved = vec![
        AgentMessage::Llm(Message::user("Hello")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::Text {
                text: "Hi there!".into(),
            }],
            stop_reason: StopReason::Stop,
            model: "mock".into(),
            provider: "mock".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }),
    ];

    let provider = MockProvider::text("ok");
    let agent = Agent::new(provider)
        .with_model("mock")
        .with_api_key("test")
        .with_messages(saved.clone());

    assert_eq!(agent.messages().len(), 2);
    assert_eq!(*agent.messages(), saved[..]);
}

#[tokio::test]
async fn test_save_and_restore_messages() {
    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    let mut rx = agent.prompt("Hi").await;
    while rx.recv().await.is_some() {}
    agent.finish().await;
    let json = agent.save_messages().expect("save should succeed");

    // Create a fresh agent and restore
    let provider2 = MockProvider::text("ok");
    let mut agent2 = Agent::new(provider2)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    agent2
        .restore_messages(&json)
        .expect("restore should succeed");
    assert_eq!(agent.messages(), agent2.messages());
}

#[tokio::test]
async fn test_agent_continues_after_restore() {
    // First agent: prompt → get response → save
    let provider1 = MockProvider::text("First response");
    let mut agent1 = Agent::new(provider1)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    let mut rx = agent1.prompt("Hello").await;
    while rx.recv().await.is_some() {}
    agent1.finish().await;
    let json = agent1.save_messages().expect("save");

    // Second agent: restore → prompt again
    // The MockProvider will receive the full restored history + new prompt
    let provider2 = MockProvider::text("Second response");
    let mut agent2 = Agent::new(provider2)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    agent2.restore_messages(&json).expect("restore");
    let mut rx = agent2.prompt("Follow up").await;
    while rx.recv().await.is_some() {}
    agent2.finish().await;

    // Should have: original user + original assistant + follow-up user + new assistant
    assert_eq!(agent2.messages().len(), 4);
    assert_eq!(agent2.messages()[0].role(), "user");
    assert_eq!(agent2.messages()[1].role(), "assistant");
    assert_eq!(agent2.messages()[2].role(), "user");
    assert_eq!(agent2.messages()[3].role(), "assistant");
}

// ---------------------------------------------------------------------------
// Real-time streaming tests (prompt_with_sender / prompt_messages_with_sender)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_prompt_with_sender_streams_events() {
    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let event_count = Arc::new(AtomicUsize::new(0));
    let count_clone = event_count.clone();

    let consumer = tokio::spawn(async move {
        while let Some(_event) = rx.recv().await {
            count_clone.fetch_add(1, Ordering::SeqCst);
        }
    });

    agent.prompt_with_sender("Hi there", tx).await;

    // tx is dropped when prompt_with_sender returns, so consumer will finish
    consumer.await.unwrap();

    assert!(event_count.load(Ordering::SeqCst) > 0);
    assert_eq!(agent.messages().len(), 2); // user + assistant
    assert!(!agent.is_streaming());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_prompt_with_sender_real_time_streaming() {
    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let received_during = Arc::new(AtomicUsize::new(0));
    let received_clone = received_during.clone();

    // On a multi-threaded runtime, the consumer can run concurrently
    let consumer = tokio::spawn(async move {
        while let Some(_event) = rx.recv().await {
            received_clone.fetch_add(1, Ordering::SeqCst);
        }
    });

    agent.prompt_with_sender("Hello", tx).await;
    consumer.await.unwrap();

    // Events were consumed by the concurrent task
    assert!(received_during.load(Ordering::SeqCst) > 0);
    assert_eq!(agent.messages().len(), 2);
}

#[tokio::test]
async fn test_prompt_messages_with_sender() {
    let provider = MockProvider::text("Response");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    let (tx, mut rx) = mpsc::unbounded_channel();

    let consumer = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    });

    let msgs = vec![AgentMessage::Llm(Message::user("Hello"))];
    agent.prompt_messages_with_sender(msgs, tx).await;

    let events = consumer.await.unwrap();
    assert!(!events.is_empty());
    assert_eq!(agent.messages().len(), 2);
}

#[tokio::test]
async fn test_continue_loop_with_sender() {
    let provider = MockProvider::text("Continued response");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    // First, add some messages to continue from (last must not be assistant)
    agent.append_message(AgentMessage::Llm(Message::user("Hello")));
    agent.append_message(AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text { text: "Hi!".into() }],
        stop_reason: StopReason::Error,
        model: "mock".into(),
        provider: "mock".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: Some("rate limited".into()),
    }));
    agent.append_message(AgentMessage::Llm(Message::user("Please try again")));

    let (tx, mut rx) = mpsc::unbounded_channel();

    let consumer = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    });

    agent.continue_loop_with_sender(tx).await;

    let events = consumer.await.unwrap();
    assert!(!events.is_empty());
    assert!(!agent.is_streaming());
}

#[tokio::test]
async fn test_prompt_with_sender_tools_restored() {
    struct DummyTool;

    #[async_trait::async_trait]
    impl AgentTool for DummyTool {
        fn name(&self) -> &str {
            "dummy"
        }
        fn label(&self) -> &str {
            "Dummy"
        }
        fn description(&self) -> &str {
            "A dummy tool"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                content: vec![Content::Text { text: "ok".into() }],
                details: serde_json::Value::Null,
            })
        }
    }

    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test")
        .with_tools(vec![Box::new(DummyTool)]);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let consumer = tokio::spawn(async move { while rx.recv().await.is_some() {} });

    agent.prompt_with_sender("Hi", tx).await;
    consumer.await.unwrap();

    // Tools should be restored after the call
    assert!(!agent.is_streaming());
    // Agent should still work for another prompt
    let mut rx2 = agent.prompt("Follow up").await;
    while rx2.recv().await.is_some() {}
    agent.finish().await;
    assert_eq!(agent.messages().len(), 4); // 2 from first + 2 from second
}
