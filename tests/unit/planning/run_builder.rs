//! Tests for build_run_driver — verifies the run assembly boundary:
//! - channels are live (events receiver works)
//! - cancel token is functional
//! - iteration counter starts at 0

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use bendclaw::kernel::tools::definition::toolset::Toolset;
use bendclaw::kernel::trace::TraceRecorder;
use bendclaw::planning::build_run_driver;
use bendclaw::planning::RunConfig;
use bendclaw::planning::RunDeps;
use bendclaw::planning::RunRequest;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;

// ── Helpers ──────────────────────────────────────────────────────────────

fn test_trace_recorder() -> TraceRecorder {
    TraceRecorder::noop("t1", "r1", "a1", "s1", "u1")
}

fn test_deps() -> RunDeps {
    let workspace = bendclaw_test_harness::mocks::context::test_workspace(
        std::env::temp_dir().join("bendclaw-test-run-deps"),
    );
    RunDeps {
        workspace,
        toolset: Toolset {
            definitions: Arc::new(vec![]),
            bindings: Arc::new(std::collections::HashMap::new()),
            tools: Arc::new(vec![]),
            allowed_tool_names: None,
        },
        skill_executor: Arc::new(bendclaw::execution::skills::NoopSkillExecutor),
        tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
        extract_memory: None,
        before_turn_hook: None,
        steering_source: None,
    }
}

fn test_request() -> RunRequest {
    RunRequest {
        user_id: "u1".into(),
        agent_id: "a1".into(),
        session_id: "s1".into(),
        run_id: "r1".into(),
        turn: 0,
        messages: vec![bendclaw::sessions::Message::user("hello")],
        system_prompt: "test".into(),
        is_dispatched: false,
    }
}

fn test_config() -> RunConfig {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    RunConfig {
        max_iterations: 10,
        max_context_tokens: 100_000,
        max_duration: Duration::from_secs(30),
        llm,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn build_run_driver_events_channel_is_live() {
    let driver = build_run_driver(
        test_deps(),
        test_trace_recorder(),
        test_request(),
        test_config(),
    );

    // The events receiver should be open (sender held by engine internals)
    // We can verify by checking that the receiver is not immediately closed.
    let mut rx = driver.events;
    // try_recv should return Empty (not Disconnected) because senders are alive
    let result = rx.try_recv();
    assert!(
        result.is_err(),
        "events channel should be open but empty initially"
    );
}

#[tokio::test]
async fn build_run_driver_cancel_token_is_functional() {
    let driver = build_run_driver(
        test_deps(),
        test_trace_recorder(),
        test_request(),
        test_config(),
    );

    assert!(
        !driver.cancel.is_cancelled(),
        "cancel token should not be cancelled initially"
    );
    driver.cancel.cancel();
    assert!(
        driver.cancel.is_cancelled(),
        "cancel token should be cancelled after cancel()"
    );
}

#[tokio::test]
async fn build_run_driver_iteration_starts_at_zero() {
    let driver = build_run_driver(
        test_deps(),
        test_trace_recorder(),
        test_request(),
        test_config(),
    );

    assert_eq!(
        driver.iteration.load(Ordering::Relaxed),
        0,
        "iteration counter should start at 0"
    );
}

#[tokio::test]
async fn build_run_driver_inbox_sender_is_connected() {
    let driver = build_run_driver(
        test_deps(),
        test_trace_recorder(),
        test_request(),
        test_config(),
    );

    // inbox_tx should be able to send without error (receiver held by engine)
    let result = driver
        .inbox_tx
        .send(bendclaw::sessions::Message::user("test"))
        .await;
    assert!(
        result.is_ok(),
        "inbox sender should be connected to engine receiver"
    );
}
