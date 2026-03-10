use anyhow::bail;
use anyhow::Result;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::stream::StreamEvent;
use bendclaw::llm::stream::ToolCallAccumulator;
use bendclaw::llm::usage::TokenUsage;
use tokio_stream::StreamExt;

// ── ResponseStream channel ──

#[tokio::test]
async fn stream_channel_text_and_done() {
    let (writer, mut stream) = ResponseStream::channel(16);

    tokio::spawn(async move {
        writer.text("hello ").await;
        writer.text("world").await;
        writer.done("stop").await;
    });

    let mut text = String::new();
    let mut done = false;
    while let Some(event) = stream.next().await {
        match event {
            StreamEvent::ContentDelta(chunk) => text.push_str(&chunk),
            StreamEvent::Done {
                finish_reason,
                provider,
                model,
            } => {
                assert_eq!(finish_reason, "stop");
                assert!(provider.is_none());
                assert!(model.is_none());
                done = true;
            }
            _ => {}
        }
    }
    assert_eq!(text, "hello world");
    assert!(done);
}

#[tokio::test]
async fn stream_channel_thinking_events() {
    let (writer, mut stream) = ResponseStream::channel(16);

    tokio::spawn(async move {
        writer.thinking("step 1").await;
        writer.thinking("step 2").await;
        writer.done("stop").await;
    });

    let mut thinking = String::new();
    while let Some(event) = stream.next().await {
        if let StreamEvent::ThinkingDelta(chunk) = event {
            thinking.push_str(&chunk);
        }
    }
    assert_eq!(thinking, "step 1step 2");
}

#[tokio::test]
async fn stream_channel_tool_calls() {
    let (writer, mut stream) = ResponseStream::channel(16);

    tokio::spawn(async move {
        writer.tool_start(0, "tc_001", "shell").await;
        writer.tool_delta(0, r#"{"command"#).await;
        writer.tool_delta(0, r#"": "ls"}"#).await;
        writer
            .tool_end(0, "tc_001", "shell", r#"{"command": "ls"}"#)
            .await;
        writer.done("tool_calls").await;
    });

    let mut deltas = String::new();
    let mut ended = false;
    while let Some(event) = stream.next().await {
        match event {
            StreamEvent::ToolCallStart { index, id, name } => {
                assert_eq!(index, 0);
                assert_eq!(id, "tc_001");
                assert_eq!(name, "shell");
            }
            StreamEvent::ToolCallDelta { json_chunk, .. } => deltas.push_str(&json_chunk),
            StreamEvent::ToolCallEnd {
                index,
                id,
                name,
                arguments,
            } => {
                assert_eq!(index, 0);
                assert_eq!(id, "tc_001");
                assert_eq!(name, "shell");
                assert_eq!(arguments, r#"{"command": "ls"}"#);
                ended = true;
            }
            _ => {}
        }
    }
    assert_eq!(deltas, r#"{"command": "ls"}"#);
    assert!(ended);
}

#[tokio::test]
async fn stream_channel_usage_event() {
    let (writer, mut stream) = ResponseStream::channel(16);

    tokio::spawn(async move {
        writer.usage(TokenUsage::new(100, 50)).await;
        writer.done("stop").await;
    });

    let mut usage_seen = false;
    while let Some(event) = stream.next().await {
        if let StreamEvent::Usage(u) = event {
            assert_eq!(u.prompt_tokens, 100);
            assert_eq!(u.completion_tokens, 50);
            assert_eq!(u.total_tokens, 150);
            usage_seen = true;
        }
    }
    assert!(usage_seen);
}

#[tokio::test]
async fn stream_done_with_provider_fields() -> Result<()> {
    let (writer, mut stream) = ResponseStream::channel(8);

    tokio::spawn(async move {
        writer
            .done_with_provider(
                "stop",
                Some("openai".to_string()),
                Some("gpt-4.1-mini".to_string()),
            )
            .await;
    });

    let event = stream.next().await;
    match event {
        Some(StreamEvent::Done {
            finish_reason,
            provider,
            model,
        }) => {
            assert_eq!(finish_reason, "stop");
            assert_eq!(provider.as_deref(), Some("openai"));
            assert_eq!(model.as_deref(), Some("gpt-4.1-mini"));
        }
        _ => bail!("expected Done event"),
    }
    Ok(())
}

#[tokio::test]
async fn stream_channel_error_event() -> Result<()> {
    let (writer, mut stream) = ResponseStream::channel(16);

    tokio::spawn(async move {
        writer.error("something broke").await;
    });

    let event = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("expected event"))?;
    match event {
        StreamEvent::Error(msg) => assert_eq!(msg, "something broke"),
        _ => bail!("expected Error event"),
    }
    Ok(())
}

#[tokio::test]
async fn stream_from_error() -> Result<()> {
    let err = bendclaw::base::ErrorCode::llm_request("test error");
    let mut stream = ResponseStream::from_error(err);

    let event = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("expected event"))?;
    match event {
        StreamEvent::Error(msg) => assert!(msg.contains("test error")),
        _ => bail!("expected Error event"),
    }
    Ok(())
}

// ── ToolCallAccumulator ──

#[test]
fn accumulator_get_or_create_grows() {
    let mut acc = ToolCallAccumulator::new();
    let tc = acc.get_or_create(2);
    tc.id = "id_2".into();
    tc.name = "tool_2".into();
    tc.arguments = "{}".into();

    assert!(acc.find(0).is_some());
    assert!(acc.find(1).is_some());
    assert!(acc.find(2).is_some());
    assert!(acc.find(3).is_none());
}

#[test]
fn accumulator_drain_filters_empty() {
    let mut acc = ToolCallAccumulator::new();
    // Index 0 and 1 created but empty
    acc.get_or_create(1);
    // Only index 2 has an id
    let tc = acc.get_or_create(2);
    tc.id = "tc_002".into();
    tc.name = "shell".into();
    tc.arguments = r#"{"cmd":"ls"}"#.into();

    let drained = acc.drain();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].id, "tc_002");
    assert_eq!(drained[0].name, "shell");
}

#[test]
fn accumulator_append_arguments() -> Result<()> {
    let mut acc = ToolCallAccumulator::new();
    let tc = acc.get_or_create(0);
    tc.id = "tc_001".into();
    tc.name = "file_read".into();
    tc.arguments.push_str(r#"{"path"#);
    tc.arguments.push_str(r#"": "a.rs"}"#);

    let found = acc
        .find(0)
        .ok_or_else(|| anyhow::anyhow!("expected entry at index 0"))?;
    assert_eq!(found.arguments, r#"{"path": "a.rs"}"#);
    Ok(())
}

#[tokio::test]
async fn stream_writer_text() -> Result<()> {
    let (writer, mut stream) = ResponseStream::channel(16);
    writer.text("hello").await;
    drop(writer);
    let event = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("expected event"))?;
    assert!(matches!(event, StreamEvent::ContentDelta(s) if s == "hello"));
    Ok(())
}

#[tokio::test]
async fn stream_writer_thinking() -> Result<()> {
    let (writer, mut stream) = ResponseStream::channel(16);
    writer.thinking("hmm").await;
    drop(writer);
    let event = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("expected event"))?;
    assert!(matches!(event, StreamEvent::ThinkingDelta(s) if s == "hmm"));
    Ok(())
}

#[tokio::test]
async fn stream_writer_tool_lifecycle() -> Result<()> {
    let (writer, mut stream) = ResponseStream::channel(16);
    writer.tool_start(0, "tc1", "shell").await;
    writer.tool_delta(0, "{\"cmd\":").await;
    writer.tool_end(0, "tc1", "shell", "{\"cmd\":\"ls\"}").await;
    drop(writer);

    let e1 = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("expected e1"))?;
    assert!(matches!(e1, StreamEvent::ToolCallStart { index: 0, .. }));
    let e2 = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("expected e2"))?;
    assert!(matches!(e2, StreamEvent::ToolCallDelta { index: 0, .. }));
    let e3 = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("expected e3"))?;
    assert!(matches!(e3, StreamEvent::ToolCallEnd { index: 0, .. }));
    Ok(())
}

#[tokio::test]
async fn stream_writer_usage() -> Result<()> {
    let (writer, mut stream) = ResponseStream::channel(16);
    writer.usage(TokenUsage::new(10, 20)).await;
    drop(writer);
    let event = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("expected event"))?;
    match event {
        StreamEvent::Usage(u) => {
            assert_eq!(u.prompt_tokens, 10);
            assert_eq!(u.completion_tokens, 20);
        }
        _ => bail!("expected Usage"),
    }
    Ok(())
}

#[tokio::test]
async fn stream_writer_done() -> Result<()> {
    let (writer, mut stream) = ResponseStream::channel(16);
    writer.done("stop").await;
    drop(writer);
    let event = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("expected event"))?;
    match event {
        StreamEvent::Done {
            finish_reason,
            provider,
            model,
        } => {
            assert_eq!(finish_reason, "stop");
            assert!(provider.is_none());
            assert!(model.is_none());
        }
        _ => bail!("expected Done"),
    }
    Ok(())
}

#[test]
fn tool_call_accumulator_get_or_create() -> Result<()> {
    let mut acc = ToolCallAccumulator::new();
    let tc = acc.get_or_create(0);
    tc.id = "tc1".into();
    tc.name = "shell".into();
    tc.arguments = "{}".into();
    let found = acc
        .find(0)
        .ok_or_else(|| anyhow::anyhow!("expected entry at index 0"))?;
    assert_eq!(found.name, "shell");
    Ok(())
}

#[test]
fn tool_call_accumulator_default() {
    let acc = ToolCallAccumulator::default();
    assert!(acc.find(0).is_none());
}

#[test]
fn accumulator_drain_empties() {
    let mut acc = ToolCallAccumulator::new();
    let tc = acc.get_or_create(0);
    tc.id = "tc_001".into();

    let first = acc.drain();
    assert_eq!(first.len(), 1);

    let second = acc.drain();
    assert!(second.is_empty());
}
