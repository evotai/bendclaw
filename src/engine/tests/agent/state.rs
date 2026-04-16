//! Tests for the Agent struct (stateful wrapper).

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use evotengine::agent::Agent;
use evotengine::provider::mock::*;
use evotengine::provider::MockProvider;
use evotengine::*;

#[tokio::test]
async fn test_agent_simple_prompt() {
    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("You are helpful.")
        .with_model("mock")
        .with_api_key("test");

    let (_handle, mut rx) = agent.submit_text("Hi there").await;

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

    let (_handle, mut rx) = agent.submit_text("Hi").await;
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
                retention: Retention::Normal,
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

    let (_handle, mut rx) = agent.submit_text("Echo hello").await;
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

    let (_handle, mut rx) = agent.submit_text("Hi").await;
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

    let (_handle, mut rx) = agent1.submit_text("Hello").await;
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
    let (_handle, mut rx) = agent2.submit_text("Follow up").await;
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
// Streaming behavior tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_submit_text_streams_events() {
    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    let (_handle, mut rx) = agent.submit_text("Hi there").await;

    let mut event_count = 0;
    while rx.recv().await.is_some() {
        event_count += 1;
    }

    agent.finish().await;

    assert!(event_count > 0);
    assert_eq!(agent.messages().len(), 2);
    assert!(!agent.is_streaming());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_submit_text_concurrent_streaming() {
    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    let (_handle, mut rx) = agent.submit_text("Hello").await;

    let received = Arc::new(AtomicUsize::new(0));
    let received_clone = received.clone();

    let consumer = tokio::spawn(async move {
        while rx.recv().await.is_some() {
            received_clone.fetch_add(1, Ordering::SeqCst);
        }
    });

    consumer.await.unwrap();
    agent.finish().await;

    assert!(received.load(Ordering::SeqCst) > 0);
    assert_eq!(agent.messages().len(), 2);
}

#[tokio::test]
async fn test_submit_messages() {
    let provider = MockProvider::text("Response");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

    let msgs = vec![AgentMessage::Llm(Message::user("Hello"))];
    let (_handle, mut rx) = agent.submit(msgs).await;

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    agent.finish().await;
    assert!(!events.is_empty());
    assert_eq!(agent.messages().len(), 2);
}

#[tokio::test]
async fn test_resume() {
    let provider = MockProvider::text("Continued response");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test");

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

    let (_handle, mut rx) = agent.resume().await;

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    agent.finish().await;
    assert!(!events.is_empty());
    assert!(!agent.is_streaming());
}

#[tokio::test]
async fn test_submit_text_tools_restored() {
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
                retention: Retention::Normal,
            })
        }
    }

    let provider = MockProvider::text("Hello!");
    let mut agent = Agent::new(provider)
        .with_system_prompt("test")
        .with_model("mock")
        .with_api_key("test")
        .with_tools(vec![Box::new(DummyTool)]);

    let (_handle, mut rx) = agent.submit_text("Hi").await;
    while rx.recv().await.is_some() {}
    agent.finish().await;

    assert!(!agent.is_streaming());

    let (_handle2, mut rx2) = agent.submit_text("Follow up").await;
    while rx2.recv().await.is_some() {}
    agent.finish().await;
    assert_eq!(agent.messages().len(), 4);
}
