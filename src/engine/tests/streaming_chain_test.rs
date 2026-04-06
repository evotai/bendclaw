use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bend_agent::Agent;
use bend_agent::AgentOptions;
use bend_agent::ContentBlock;
use bend_agent::Message;
use bend_agent::MessageRole;
use bend_agent::ProviderKind;
use bend_agent::SDKMessage;
use bend_agent::Tool;
use bend_agent::ToolError;
use bend_agent::ToolInputSchema;
use bend_agent::ToolResult;
use bend_agent::ToolUseContext;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

type TestResult = std::result::Result<(), Box<dyn Error>>;

struct ResponseSpec {
    status_line: &'static str,
    content_type: &'static str,
    body: String,
    delay_ms: u64,
}

async fn spawn_sequence_server(responses: Vec<ResponseSpec>) -> Result<String, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;

    tokio::spawn(async move {
        for response in responses {
            let accepted = listener.accept().await;
            let (mut stream, _) = match accepted {
                Ok(parts) => parts,
                Err(_) => return,
            };

            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).await;
                let read = match read {
                    Ok(read) => read,
                    Err(_) => return,
                };

                if read == 0 {
                    break;
                }

                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let response_text = format!(
                "HTTP/1.1 {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.status_line,
                response.content_type,
                response.body.len(),
                response.body
            );
            if response.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(response.delay_ms)).await;
            }
            let _ = stream.write_all(response_text.as_bytes()).await;
        }
    });

    Ok(format!("http://{address}"))
}

fn openai_text_stream(chunks: &[&str]) -> String {
    let mut lines = chunks
        .iter()
        .map(|chunk| format!(r#"data: {{"choices":[{{"delta":{{"content":"{chunk}"}}}}]}}"#))
        .collect::<Vec<_>>();
    lines.push(
        r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":2}}"#
            .to_string(),
    );
    lines.push("data: [DONE]".to_string());
    lines.join("\n")
}

fn anthropic_tool_use_stream() -> String {
    [
        r#"data: {"type":"message_start","message":{"usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"EchoTool","input":{}}}"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"value\":\"ping\"}"}}"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}"#,
        "data: [DONE]",
    ]
    .join("\n")
}

fn anthropic_text_stream(chunks: &[&str], input_tokens: u64, output_tokens: u64) -> String {
    let mut lines = vec![format!(
        r#"data: {{"type":"message_start","message":{{"usage":{{"input_tokens":{input_tokens},"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
    )];
    lines.push(
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
            .to_string(),
    );
    for chunk in chunks {
        lines.push(format!(
            r#"data: {{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{chunk}"}}}}"#
        ));
    }
    lines.push(r#"data: {"type":"content_block_stop","index":0}"#.to_string());
    lines.push(format!(
        r#"data: {{"type":"message_delta","delta":{{"stop_reason":"end_turn"}},"usage":{{"output_tokens":{output_tokens}}}}}"#
    ));
    lines.push("data: [DONE]".to_string());
    lines.join("\n")
}

async fn collect_query_messages(agent: &mut Agent, prompt: &str) -> Vec<SDKMessage> {
    let (mut rx, handle) = agent.query(prompt).await;
    let mut messages = Vec::new();

    while let Some(message) = rx.recv().await {
        messages.push(message);
    }

    let final_messages = handle.await.unwrap();
    agent.messages = final_messages;
    messages
}

fn assistant_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "EchoTool"
    }

    fn description(&self) -> &str {
        "Echoes the provided value"
    }

    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema::default()
    }

    async fn call(&self, input: Value, _ctx: &ToolUseContext) -> Result<ToolResult, ToolError> {
        tokio::time::sleep(Duration::from_millis(15)).await;
        let value = input
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("missing");
        Ok(ToolResult::text(format!("echo: {value}")))
    }
}

#[tokio::test]
async fn streamed_text_query_emits_partial_then_final_messages() -> TestResult {
    let base_url = spawn_sequence_server(vec![ResponseSpec {
        status_line: "200 OK",
        content_type: "text/event-stream",
        body: openai_text_stream(&["pon", "g"]),
        delay_ms: 0,
    }])
    .await?;

    let temp_dir = tempfile::tempdir()?;
    let mut agent = Agent::new(AgentOptions {
        provider: Some(ProviderKind::OpenAi),
        api_key: Some("test-key".to_string()),
        base_url: Some(base_url),
        model: Some("gpt-4o".to_string()),
        cwd: Some(temp_dir.path().to_string_lossy().to_string()),
        allowed_tools: Some(vec![]),
        ..Default::default()
    })
    .await?;

    let messages = collect_query_messages(&mut agent, "ping").await;
    agent.close().await;

    let partials = messages
        .iter()
        .filter_map(|message| match message {
            SDKMessage::PartialMessage { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(!partials.is_empty());
    assert_eq!(partials.join(""), "pong");

    let assistant = messages
        .iter()
        .find_map(|message| match message {
            SDKMessage::Assistant { message, .. } => Some(message.clone()),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("missing assistant message"))?;

    assert_eq!(assistant.role, MessageRole::Assistant);
    assert_eq!(assistant_text(&assistant), "pong");

    let result = messages
        .iter()
        .find_map(|message| match message {
            SDKMessage::Result {
                text,
                usage,
                summary,
                ..
            } => Some((text.clone(), usage.clone(), summary.clone())),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("missing result message"))?;

    assert_eq!(result.0, "pong");
    assert_eq!(result.1.input_tokens, 1);
    assert_eq!(result.1.output_tokens, 2);
    assert_eq!(result.2.stream.request_count, 1);
    assert!(result.2.stream.first_ttfb_ms.is_some());
    assert!(result.2.stream.first_ttft_ms.is_some());
    assert!(result.2.stream.total_chunk_count > 0);
    assert!(result.2.stream.total_bytes_received > 0);

    let assistant_index = messages
        .iter()
        .position(|message| matches!(message, SDKMessage::Assistant { .. }))
        .ok_or_else(|| std::io::Error::other("missing assistant index"))?;
    let partial_index = messages
        .iter()
        .position(|message| matches!(message, SDKMessage::PartialMessage { .. }))
        .ok_or_else(|| std::io::Error::other("missing partial index"))?;

    assert!(partial_index < assistant_index);

    Ok(())
}

#[tokio::test]
async fn streamed_tool_loop_accumulates_summary_across_turns() -> TestResult {
    let base_url = spawn_sequence_server(vec![
        ResponseSpec {
            status_line: "200 OK",
            content_type: "text/event-stream",
            body: anthropic_tool_use_stream(),
            delay_ms: 0,
        },
        ResponseSpec {
            status_line: "200 OK",
            content_type: "text/event-stream",
            body: anthropic_text_stream(&["tool says ", "pong"], 3, 2),
            delay_ms: 0,
        },
    ])
    .await?;

    let temp_dir = tempfile::tempdir()?;
    let mut agent = Agent::new(AgentOptions {
        provider: Some(ProviderKind::Anthropic),
        api_key: Some("test-key".to_string()),
        base_url: Some(base_url),
        model: Some("claude-sonnet-4-6-20250514".to_string()),
        cwd: Some(temp_dir.path().to_string_lossy().to_string()),
        allowed_tools: Some(vec!["EchoTool".to_string()]),
        custom_tools: vec![Arc::new(EchoTool)],
        ..Default::default()
    })
    .await?;

    let messages = collect_query_messages(&mut agent, "use the tool").await;
    agent.close().await;

    let assistants = messages
        .iter()
        .filter_map(|message| match message {
            SDKMessage::Assistant { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(assistants.len(), 2);
    assert!(assistants[0]
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse { name, .. } if name == "EchoTool")));
    assert_eq!(assistant_text(&assistants[1]), "tool says pong");

    let tool_result = messages
        .iter()
        .find_map(|message| match message {
            SDKMessage::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => Some((tool_name.clone(), content.clone(), *is_error)),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("missing tool result"))?;

    assert_eq!(tool_result.0, "EchoTool");
    assert_eq!(tool_result.1, "echo: ping");
    assert!(!tool_result.2);

    let result = messages
        .iter()
        .find_map(|message| match message {
            SDKMessage::Result {
                text,
                usage,
                summary,
                ..
            } => Some((text.clone(), usage.clone(), summary.clone())),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("missing result message"))?;

    assert_eq!(result.0, "tool says pong");
    assert_eq!(result.1.input_tokens, 5);
    assert_eq!(result.1.output_tokens, 5);
    assert_eq!(result.2.stream.request_count, 2);
    assert!(result.2.stream.first_ttfb_ms.is_some());
    assert!(result.2.stream.first_ttft_ms.is_some());
    assert!(result.2.stream.total_chunk_count > 0);
    assert!(result.2.stream.total_bytes_received > 0);
    assert!(result.2.tool_duration_ms >= 10);

    let tool_result_index = messages
        .iter()
        .position(|message| matches!(message, SDKMessage::ToolResult { .. }))
        .ok_or_else(|| std::io::Error::other("missing tool result index"))?;
    let final_assistant_index = messages
        .iter()
        .rposition(|message| matches!(message, SDKMessage::Assistant { .. }))
        .ok_or_else(|| std::io::Error::other("missing final assistant index"))?;

    assert!(tool_result_index < final_assistant_index);

    Ok(())
}

#[tokio::test]
async fn streamed_summary_reports_non_zero_ttfb_and_ttft_after_upstream_delay() -> TestResult {
    let base_url = spawn_sequence_server(vec![ResponseSpec {
        status_line: "200 OK",
        content_type: "text/event-stream",
        body: openai_text_stream(&["pong"]),
        delay_ms: 35,
    }])
    .await?;

    let temp_dir = tempfile::tempdir()?;
    let mut agent = Agent::new(AgentOptions {
        provider: Some(ProviderKind::OpenAi),
        api_key: Some("test-key".to_string()),
        base_url: Some(base_url),
        model: Some("gpt-4o".to_string()),
        cwd: Some(temp_dir.path().to_string_lossy().to_string()),
        allowed_tools: Some(vec![]),
        ..Default::default()
    })
    .await?;

    let messages = collect_query_messages(&mut agent, "ping").await;
    agent.close().await;

    let summary = messages
        .iter()
        .find_map(|message| match message {
            SDKMessage::Result { summary, .. } => Some(summary.clone()),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("missing result summary"))?;

    assert!(summary.stream.first_ttfb_ms.unwrap_or_default() >= 20);
    assert!(summary.stream.first_ttft_ms.unwrap_or_default() >= 20);

    Ok(())
}

#[tokio::test]
async fn query_handle_returns_complete_conversation_history() -> TestResult {
    let base_url = spawn_sequence_server(vec![
        ResponseSpec {
            status_line: "200 OK",
            content_type: "text/event-stream",
            body: anthropic_tool_use_stream(),
            delay_ms: 0,
        },
        ResponseSpec {
            status_line: "200 OK",
            content_type: "text/event-stream",
            body: anthropic_text_stream(&["done"], 3, 1),
            delay_ms: 0,
        },
    ])
    .await?;

    let temp_dir = tempfile::tempdir()?;
    let mut agent = Agent::new(AgentOptions {
        provider: Some(ProviderKind::Anthropic),
        api_key: Some("test-key".to_string()),
        base_url: Some(base_url),
        model: Some("claude-sonnet-4-6-20250514".to_string()),
        cwd: Some(temp_dir.path().to_string_lossy().to_string()),
        allowed_tools: Some(vec!["EchoTool".to_string()]),
        custom_tools: vec![Arc::new(EchoTool)],
        ..Default::default()
    })
    .await?;

    let (mut rx, handle) = agent.query("use the tool").await;
    while rx.recv().await.is_some() {}
    let final_messages = handle.await?;
    agent.close().await;

    // Should have: User, Assistant(tool_use), User(tool_result), Assistant(text)
    assert_eq!(
        final_messages.len(),
        4,
        "expected 4 messages, got {}",
        final_messages.len()
    );
    assert_eq!(final_messages[0].role, MessageRole::User);
    assert_eq!(final_messages[1].role, MessageRole::Assistant);
    assert_eq!(final_messages[2].role, MessageRole::User);
    assert_eq!(final_messages[3].role, MessageRole::Assistant);

    // First assistant has tool_use
    assert!(final_messages[1]
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolUse { .. })));

    // Second user has tool_result
    assert!(final_messages[2]
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolResult { .. })));

    // Final assistant has text
    assert_eq!(assistant_text(&final_messages[3]), "done");

    Ok(())
}

#[tokio::test]
async fn multi_turn_query_preserves_conversation_history() -> TestResult {
    let base_url = spawn_sequence_server(vec![
        // Turn 1: tool use then text response
        ResponseSpec {
            status_line: "200 OK",
            content_type: "text/event-stream",
            body: anthropic_tool_use_stream(),
            delay_ms: 0,
        },
        ResponseSpec {
            status_line: "200 OK",
            content_type: "text/event-stream",
            body: anthropic_text_stream(&["first answer"], 3, 2),
            delay_ms: 0,
        },
        // Turn 2: text-only response
        ResponseSpec {
            status_line: "200 OK",
            content_type: "text/event-stream",
            body: anthropic_text_stream(&["second answer"], 5, 2),
            delay_ms: 0,
        },
    ])
    .await?;

    let temp_dir = tempfile::tempdir()?;
    let mut agent = Agent::new(AgentOptions {
        provider: Some(ProviderKind::Anthropic),
        api_key: Some("test-key".to_string()),
        base_url: Some(base_url),
        model: Some("claude-sonnet-4-6-20250514".to_string()),
        cwd: Some(temp_dir.path().to_string_lossy().to_string()),
        allowed_tools: Some(vec!["EchoTool".to_string()]),
        custom_tools: vec![Arc::new(EchoTool)],
        ..Default::default()
    })
    .await?;

    // Turn 1
    let _ = collect_query_messages(&mut agent, "first question").await;

    // After turn 1, agent.messages should have 4 entries
    assert_eq!(
        agent.messages.len(),
        4,
        "turn 1: expected 4 messages, got {}",
        agent.messages.len()
    );

    // Turn 2
    let _ = collect_query_messages(&mut agent, "second question").await;

    // After turn 2: 4 (turn 1) + User + Assistant = 6
    assert_eq!(
        agent.messages.len(),
        6,
        "turn 2: expected 6 messages, got {}",
        agent.messages.len()
    );

    // Verify roles alternate correctly — no consecutive User messages
    let roles: Vec<_> = agent.messages.iter().map(|m| &m.role).collect();
    assert_eq!(*roles[0], MessageRole::User);
    assert_eq!(*roles[1], MessageRole::Assistant);
    assert_eq!(*roles[2], MessageRole::User); // tool result
    assert_eq!(*roles[3], MessageRole::Assistant); // "first answer"
    assert_eq!(*roles[4], MessageRole::User); // "second question"
    assert_eq!(*roles[5], MessageRole::Assistant); // "second answer"

    // Verify final text
    assert_eq!(assistant_text(&agent.messages[5]), "second answer");

    agent.close().await;
    Ok(())
}
