//! Engine behaviour tests — drive the full agent loop via Session::chat().
//!
//! Engine, Context, and TraceRecorder are pub(crate), so we exercise the engine
//! indirectly through the public Session API, exactly as the session tests do.
//! These tests require a live Databend connection (same as all `it` tests).

use std::sync::Arc;

use anyhow::Result;

use crate::mocks::context::test_session;
use crate::mocks::llm::MockLLMProvider;
use crate::mocks::llm::MockTurn;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn engine_text_response_returns_end_turn() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("Hello from engine!"));
    let session = test_session(llm).await?;
    let text = session.chat("hi", "").await?.finish().await?;
    assert_eq!(text, "Hello from engine!");
    Ok(())
}

#[tokio::test]
async fn engine_empty_text_response_is_ok() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text(""));
    let session = test_session(llm).await?;
    let text = session.chat("hi", "").await?.finish().await?;
    assert_eq!(text, "");
    Ok(())
}

#[tokio::test]
async fn engine_tool_then_text_completes_successfully() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo hi"}"#.into(),
        },
        MockTurn::Text("Done!".into()),
    ]));
    let session = test_session(llm).await?;
    let text = session.chat("run echo", "").await?.finish().await?;
    assert_eq!(text, "Done!");
    Ok(())
}

#[tokio::test]
async fn engine_multiple_tool_calls_in_single_turn() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCalls(vec![
            ("shell".into(), r#"{"command":"echo a"}"#.into()),
            ("shell".into(), r#"{"command":"echo b"}"#.into()),
        ]),
        MockTurn::Text("Both done.".into()),
    ]));
    let session = test_session(llm).await?;
    let text = session.chat("run two commands", "").await?.finish().await?;
    assert_eq!(text, "Both done.");
    Ok(())
}

#[tokio::test]
async fn engine_stops_at_max_iterations() -> Result<()> {
    // Always returns a tool call — the engine must stop at max_iterations.
    let llm = Arc::new(MockLLMProvider::always_tool_call(
        "shell",
        r#"{"command":"echo hi"}"#,
    ));
    let session = test_session(llm).await?;
    // Should not hang — max_iterations (default 20) will stop it.
    let _text = session.chat("loop forever", "").await?.finish().await?;
    Ok(())
}

#[tokio::test]
async fn engine_cancel_stops_run() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::always_tool_call(
        "shell",
        r#"{"command":"echo hi"}"#,
    ));
    let session = test_session(llm).await?;
    let stream = session.chat("start", "").await?;
    // Cancel immediately.
    session.cancel_current();
    // finish() should return without hanging.
    let _text = stream.finish().await?;
    Ok(())
}

#[tokio::test]
async fn engine_second_chat_on_same_session_works() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::Text("first".into()),
        MockTurn::Text("second".into()),
    ]));
    let session = test_session(llm).await?;

    let t1 = session.chat("turn 1", "").await?.finish().await?;
    let t2 = session.chat("turn 2", "").await?.finish().await?;

    assert_eq!(t1, "first");
    assert_eq!(t2, "second");
    Ok(())
}

#[tokio::test]
async fn engine_concurrent_run_rejected() -> Result<()> {
    use std::time::Duration;

    let llm = Arc::new(MockLLMProvider::always_tool_call(
        "shell",
        r#"{"command":"sleep 1"}"#,
    ));
    let session = Arc::new(test_session(llm).await?);

    // Start a run.
    let stream1 = session.chat("first", "").await?;

    // A second concurrent run must be rejected.
    let result2 = session.chat("second", "").await;
    assert!(
        result2.is_err(),
        "expected second concurrent run to be rejected"
    );

    // Clean up the first run.
    session.cancel_current();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = stream1.finish().await;
    Ok(())
}

#[tokio::test]
async fn engine_session_is_idle_after_run() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let session = test_session(llm).await?;

    assert!(session.is_idle());
    let stream = session.chat("hi", "").await?;
    // While running, session is not idle.
    assert!(session.is_running());
    stream.finish().await?;
    // After finish, session returns to idle.
    assert!(session.is_idle());
    Ok(())
}

#[tokio::test]
async fn engine_run_id_is_nonempty() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let session = test_session(llm).await?;
    let stream = session.chat("hi", "").await?;
    let run_id = stream.run_id().to_string();
    stream.finish().await?;
    assert!(!run_id.is_empty());
    Ok(())
}

#[tokio::test]
async fn engine_three_turn_conversation() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo step1"}"#.into(),
        },
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo step2"}"#.into(),
        },
        MockTurn::Text("All steps done.".into()),
    ]));
    let session = test_session(llm).await?;
    let text = session.chat("run three steps", "").await?.finish().await?;
    assert_eq!(text, "All steps done.");
    Ok(())
}

#[tokio::test]
async fn engine_cancel_run_by_id_stops_run() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::always_tool_call(
        "shell",
        r#"{"command":"echo hi"}"#,
    ));
    let session = test_session(llm).await?;
    let stream = session.chat("start", "").await?;
    let run_id = stream.run_id().to_string();

    // Cancel by run_id.
    let cancelled = session.cancel_run(&run_id);
    assert!(
        cancelled,
        "expected cancel_run to return true for active run"
    );

    let _ = stream.finish().await;
    Ok(())
}

#[tokio::test]
async fn engine_cancel_wrong_run_id_returns_false() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let session = test_session(llm).await?;
    let stream = session.chat("hi", "").await?;

    let cancelled = session.cancel_run("nonexistent-run-id");
    assert!(!cancelled);

    stream.finish().await?;
    Ok(())
}

#[tokio::test]
async fn engine_session_info_reflects_state() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let session = test_session(llm).await?;

    let info_before = session.info();
    assert_eq!(info_before.status, "idle");

    let stream = session.chat("hi", "").await?;
    let info_running = session.info();
    assert_eq!(info_running.status, "running");

    stream.finish().await?;
    let info_after = session.info();
    assert_eq!(info_after.status, "idle");
    Ok(())
}

#[tokio::test]
async fn engine_belongs_to_correct_agent_and_user() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let session = test_session(llm).await?;

    // test_session uses agent_id="a1", user_id="u1"
    assert!(session.belongs_to("a1", "u1"));
    assert!(!session.belongs_to("other", "u1"));
    assert!(!session.belongs_to("a1", "other"));
    Ok(())
}

#[tokio::test]
async fn engine_close_cancels_and_idles_session() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::always_tool_call(
        "shell",
        r#"{"command":"echo hi"}"#,
    ));
    let session = test_session(llm).await?;
    let stream = session.chat("start", "").await?;

    session.close().await;
    assert!(session.is_idle());

    let _ = stream.finish().await;
    Ok(())
}
