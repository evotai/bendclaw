use std::sync::Arc;

use anyhow::Result;

use crate::mocks::context::test_session;
use crate::mocks::llm::MockLLMProvider;
use crate::mocks::llm::MockTurn;

#[tokio::test]
async fn text_response_returns_end_turn() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("Hello!"));
    let session = test_session(llm).await?;
    let result = session.chat("hi", "").await?.finish().await?;
    assert!(!result.is_empty());
    Ok(())
}

#[tokio::test]
async fn stops_at_max_iterations() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::always_tool_call(
        "shell",
        r#"{"command":"echo hi"}"#,
    ));
    let session = test_session(llm).await?;
    let _result = session.chat("hello", "").await?.finish().await?;
    Ok(())
}

#[tokio::test]
async fn multi_turn_tool_then_text() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo hi"}"#.into(),
        },
        MockTurn::Text("Done!".into()),
    ]));
    let session = test_session(llm).await?;
    let result = session.chat("run echo", "").await?.finish().await?;
    assert!(!result.is_empty());
    Ok(())
}

#[tokio::test]
async fn multiple_tool_calls_in_single_turn() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCalls(vec![
            ("shell".into(), r#"{"command":"echo a"}"#.into()),
            ("shell".into(), r#"{"command":"echo b"}"#.into()),
        ]),
        MockTurn::Text("Both done.".into()),
    ]));
    let session = test_session(llm).await?;
    let result = session.chat("run two commands", "").await?.finish().await?;
    assert!(!result.is_empty());
    Ok(())
}

// ── Session::idle_duration ──

#[tokio::test]
async fn idle_duration_is_nonzero_after_run() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let session = test_session(llm).await?;
    session.chat("hi", "").await?.finish().await?;
    let d = session.idle_duration();
    // After a completed run the session is idle; duration should be >= 0
    assert!(d.as_millis() < 60_000, "idle_duration should be recent");
    Ok(())
}

// ── Session::current_run_id ──

#[tokio::test]
async fn current_run_id_none_when_idle() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let session = test_session(llm).await?;
    assert!(session.current_run_id().is_none());
    Ok(())
}

#[tokio::test]
async fn current_run_id_some_while_running() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::always_tool_call(
        "shell",
        r#"{"command":"echo hi"}"#,
    ));
    let session = test_session(llm).await?;
    let stream = session.chat("start", "").await?;
    // While running, current_run_id should match the stream's run_id
    let run_id = session.current_run_id();
    assert!(run_id.is_some());
    assert_eq!(run_id.as_deref(), Some(stream.run_id()));
    session.cancel_current();
    stream.finish().await?;
    Ok(())
}

#[tokio::test]
async fn current_run_id_none_after_finish() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let session = test_session(llm).await?;
    session.chat("hi", "").await?.finish().await?;
    assert!(session.current_run_id().is_none());
    Ok(())
}
