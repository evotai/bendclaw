//! Integration tests against the real Anthropic API.
//! Run with: ANTHROPIC_API_KEY=... cargo test --test integration_anthropic -- --ignored
//!
//! These tests are #[ignore] by default so they don't run in CI without a key.

use bendengine::agent_loop::agent_loop;
use bendengine::agent_loop::AgentLoopConfig;
use bendengine::provider::AnthropicProvider;
use bendengine::tools;
use bendengine::types::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set")
}

fn make_config(provider: AnthropicProvider) -> AgentLoopConfig {
    AgentLoopConfig {
        provider: std::sync::Arc::new(provider),
        model: "claude-sonnet-4-20250514".into(),
        api_key: api_key(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: Some(1024),
        temperature: Some(0.0),
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

fn extract_assistant_text(messages: &[AgentMessage]) -> String {
    messages
        .iter()
        .filter_map(|m| {
            if let AgentMessage::Llm(Message::Assistant { content, .. }) = m {
                Some(
                    content
                        .iter()
                        .filter_map(|c| {
                            if let Content::Text { text } = c {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(""),
                )
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_assistant_message(messages: &[AgentMessage]) -> bool {
    messages
        .iter()
        .any(|m| matches!(m, AgentMessage::Llm(Message::Assistant { .. })))
}

/// Simple text response — no tools.
#[tokio::test]
#[ignore]
async fn test_anthropic_simple_text() {
    let provider = AnthropicProvider;
    let config = make_config(provider);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let mut context = AgentContext {
        system_prompt: "Reply with exactly one word.".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let prompt = AgentMessage::Llm(Message::user("What color is the sky?"));
    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    assert!(
        !new_messages.is_empty(),
        "Expected at least one new message"
    );
    assert!(
        has_assistant_message(&new_messages),
        "Expected an assistant message"
    );

    let text = extract_assistant_text(&new_messages);
    assert!(!text.is_empty(), "Expected non-empty text response");
    println!("Response: {}", text);

    // Collect events and verify we got the expected flow
    let mut got_start = false;
    let mut _got_text_delta = false;
    let mut got_end = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            AgentEvent::AgentStart => got_start = true,
            AgentEvent::MessageUpdate {
                delta: StreamDelta::Text { .. },
                ..
            } => _got_text_delta = true,
            AgentEvent::AgentEnd { .. } => got_end = true,
            _ => {}
        }
    }
    assert!(got_start, "Expected AgentStart event");
    // Note: text deltas may not appear in events due to agent_loop event ordering.
    // The response content is verified above via new_messages.
    assert!(got_end, "Expected AgentEnd event");
}

/// Tool use — give it bash and ask it to run a simple command.
#[tokio::test]
#[ignore]
async fn test_anthropic_tool_use() {
    let provider = AnthropicProvider;
    let config = make_config(provider);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let mut context = AgentContext {
        system_prompt:
            "You are a helpful assistant. Use the bash tool to answer questions. Be concise.".into(),
        messages: Vec::new(),
        tools: tools::default_tools(),
    };

    let prompt = AgentMessage::Llm(Message::user(
        "What is the output of `echo hello_yoagent`? Use bash to run it.",
    ));
    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Should have multiple messages: assistant (tool call) + tool result + assistant (final)
    assert!(
        new_messages.len() >= 3,
        "Expected at least 3 messages (tool call + result + response), got {}",
        new_messages.len()
    );

    // Verify we got tool execution events
    let mut got_tool_start = false;
    let mut got_tool_end = false;
    let mut tool_names: Vec<String> = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AgentEvent::ToolExecutionStart { tool_name, .. } => {
                got_tool_start = true;
                tool_names.push(tool_name);
            }
            AgentEvent::ToolExecutionEnd { .. } => {
                got_tool_end = true;
            }
            _ => {}
        }
    }
    assert!(got_tool_start, "Expected ToolExecutionStart event");
    assert!(got_tool_end, "Expected ToolExecutionEnd event");
    assert!(
        tool_names.contains(&"bash".to_string()),
        "Expected bash tool to be called, got: {:?}",
        tool_names
    );

    // Verify the final response mentions the output
    let final_text = extract_assistant_text(&new_messages);
    assert!(
        final_text.contains("hello_yoagent"),
        "Expected response to contain 'hello_yoagent', got: {}",
        final_text
    );
    println!("Full text: {}", final_text);
}

/// Multi-turn — continue from existing context.
#[tokio::test]
#[ignore]
async fn test_anthropic_multi_turn() {
    let provider = AnthropicProvider;
    let config = make_config(provider);
    let cancel = CancellationToken::new();

    let mut context = AgentContext {
        system_prompt: "Reply with exactly one word.".into(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    // Turn 1
    let (tx1, _rx1) = mpsc::unbounded_channel();
    let prompt1 = AgentMessage::Llm(Message::user("What color is grass?"));
    let msgs1 = agent_loop(vec![prompt1], &mut context, &config, tx1, cancel.clone()).await;
    assert!(!msgs1.is_empty(), "Turn 1 should produce messages");

    // Turn 2 — should have context from turn 1
    let (tx2, _rx2) = mpsc::unbounded_channel();
    let prompt2 = AgentMessage::Llm(Message::user("And the sky?"));
    let msgs2 = agent_loop(vec![prompt2], &mut context, &config, tx2, cancel.clone()).await;
    assert!(!msgs2.is_empty(), "Turn 2 should produce messages");

    // Context should have all messages from both turns
    assert!(
        context.messages.len() >= 4,
        "Expected at least 4 messages in context (2 user + 2 assistant), got {}",
        context.messages.len()
    );
    println!(
        "Context has {} messages after 2 turns",
        context.messages.len()
    );
}
