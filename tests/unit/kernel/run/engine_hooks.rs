use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bendclaw::kernel::execution::CallExecutor;
use bendclaw::kernel::run::compaction::Compactor;
use bendclaw::kernel::run::context::Context;
use bendclaw::kernel::run::engine::Engine;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::run::hooks::BeforeTurnHook;
use bendclaw::kernel::run::hooks::SteeringDecision;
use bendclaw::kernel::run::hooks::SteeringSource;
use bendclaw::kernel::run::hooks::TurnDecision;
use bendclaw::kernel::run::result::Reason;
use bendclaw::kernel::tools::progressive::ProgressiveToolView;
use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw::kernel::tools::ToolRuntime;
use bendclaw::kernel::trace::TraceRecorder;
use bendclaw::kernel::Message;
use bendclaw::storage::dal::trace::repo::SpanRepo;
use bendclaw::storage::dal::trace::repo::TraceRepo;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use tokio_util::sync::CancellationToken;

// ── Mock hooks ──

struct AbortHook;

#[async_trait]
impl BeforeTurnHook for AbortHook {
    async fn before_turn(&self, _iteration: u32, _messages: &[Message]) -> TurnDecision {
        TurnDecision::Abort("test abort".into())
    }
}

struct InjectHook {
    injected: Vec<Message>,
}

#[async_trait]
impl BeforeTurnHook for InjectHook {
    async fn before_turn(&self, iteration: u32, _messages: &[Message]) -> TurnDecision {
        if iteration == 1 {
            TurnDecision::InjectMessages(self.injected.clone())
        } else {
            TurnDecision::Continue
        }
    }
}

// ── Helpers ──

fn test_trace_recorder() -> TraceRecorder {
    let pool = bendclaw_test_harness::mocks::context::dummy_pool();
    TraceRecorder::with_writer(
        bendclaw::kernel::trace::TraceWriter::noop(),
        Arc::new(TraceRepo::new(pool.clone())),
        Arc::new(SpanRepo::new(pool)),
        "trace-1",
        "run-1",
        "agent-1",
        "session-1",
        "user-1",
    )
}

struct NoopSkillExecutor;

#[async_trait]
impl bendclaw::kernel::skills::executor::SkillExecutor for NoopSkillExecutor {
    async fn execute(
        &self,
        _skill_name: &str,
        _args: &[String],
    ) -> bendclaw::base::Result<bendclaw::kernel::skills::executor::SkillOutput> {
        Ok(bendclaw::kernel::skills::executor::SkillOutput {
            data: None,
            error: Some("not implemented".into()),
        })
    }
}

fn build_engine(
    llm: Arc<MockLLMProvider>,
    messages: Vec<Message>,
) -> (Engine, tokio::sync::mpsc::Receiver<Event>) {
    let cancel = CancellationToken::new();
    let (tx, rx) = Engine::create_channel();
    let (_inbox_tx, inbox_rx) = Engine::create_inbox();

    let workspace = bendclaw_test_harness::mocks::context::test_workspace(
        std::env::temp_dir().join("bendclaw-engine-hooks-test"),
    );

    let executor = CallExecutor::new(
        Arc::new(ToolRegistry::new()),
        Arc::new(NoopSkillExecutor),
        bendclaw::kernel::tools::ToolContext {
            user_id: "user-1".into(),
            session_id: "session-1".into(),
            agent_id: "agent-1".into(),
            run_id: "run-1".into(),
            trace_id: "trace-1".into(),
            workspace,
            is_dispatched: false,
            runtime: ToolRuntime {
                event_tx: None,
                cancel: cancel.clone(),
                tool_call_id: None,
            },
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
        },
        cancel.clone(),
        tx.clone(),
    );

    let ctx = Context {
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        session_id: "session-1".into(),
        run_id: "run-1".into(),
        turn: 0,
        trace_id: "trace-1".into(),
        llm: llm.clone(),
        model: "mock".into(),
        temperature: 0.0,
        max_iterations: 10,
        max_context_tokens: 250_000,
        max_duration: Duration::from_secs(30),
        tool_view: ProgressiveToolView::new(Arc::new(vec![])),
        system_prompt: "You are a test assistant.".into(),
        messages,
    };

    let compactor = Compactor::new(llm, "mock".into(), cancel.clone());
    let trace = test_trace_recorder();

    let engine = Engine::from_tx(
        ctx,
        executor,
        compactor,
        cancel,
        Arc::new(AtomicU32::new(0)),
        trace,
        tx,
        inbox_rx,
        None,
    );

    (engine, rx)
}

// ── Tests ──

/// Verifies that Engine actually calls BeforeTurnHook and aborts when
/// the hook returns TurnDecision::Abort.
#[tokio::test]
async fn engine_aborts_on_before_turn_hook() {
    let llm = Arc::new(MockLLMProvider::with_text("should not reach LLM"));
    let messages = vec![Message::user("hello")];
    let (engine, _rx) = build_engine(llm, messages);

    let mut engine = engine.with_before_turn(Box::new(AbortHook));
    let result = engine.run().await.expect("engine should not error");

    assert!(
        matches!(result.stop_reason, Reason::Aborted),
        "expected Aborted, got {:?}",
        result.stop_reason
    );
}

/// Verifies that Engine calls BeforeTurnHook and injects messages
/// into the context before the LLM call.
#[tokio::test]
async fn engine_injects_messages_from_before_turn_hook() {
    let llm = Arc::new(MockLLMProvider::with_text("got it"));
    let messages = vec![Message::user("hello")];
    let (engine, mut rx) = build_engine(llm, messages);

    let mut engine = engine.with_before_turn(Box::new(InjectHook {
        injected: vec![Message::user("injected context")],
    }));

    let result = engine.run().await.expect("engine should not error");

    // Engine should complete normally (not abort)
    assert!(
        !matches!(result.stop_reason, Reason::Aborted),
        "engine should not have aborted"
    );

    // Drain events and check that the injected message was processed
    let mut saw_turn_start = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, Event::TurnStart { .. }) {
            saw_turn_start = true;
        }
    }
    assert!(
        saw_turn_start,
        "engine should have started at least one turn"
    );

    // The injected message should be in the result messages
    assert!(
        result
            .messages
            .iter()
            .any(|m| m.text() == "injected context"),
        "injected message should appear in result messages"
    );
}

struct RedirectOnceSource {
    messages: Vec<Message>,
}

#[async_trait]
impl SteeringSource for RedirectOnceSource {
    async fn check_steering(&self, iteration: u32) -> SteeringDecision {
        if iteration == 1 {
            SteeringDecision::Redirect(self.messages.clone())
        } else {
            SteeringDecision::Continue
        }
    }
}

/// Verifies that Engine calls SteeringSource after tool dispatch and
/// injects redirected messages + emits MessageInjected events.
#[tokio::test]
async fn engine_steering_source_injects_after_tools() {
    // LLM returns a tool call on turn 1, then a text response on turn 2
    let llm = Arc::new(MockLLMProvider::new(vec![
        bendclaw_test_harness::mocks::llm::MockTurn::ToolCall {
            name: "nonexistent_tool".into(),
            arguments: "{}".into(),
        },
        bendclaw_test_harness::mocks::llm::MockTurn::Text("final answer".into()),
    ]));
    let messages = vec![Message::user("hello")];
    let (engine, mut rx) = build_engine(llm, messages);

    let mut engine = engine.with_steering(Box::new(RedirectOnceSource {
        messages: vec![Message::user("steering redirect")],
    }));

    let result = engine.run().await.expect("engine should not error");

    // The steered message should appear in result messages
    assert!(
        result
            .messages
            .iter()
            .any(|m| m.text() == "steering redirect"),
        "steering message should appear in result messages"
    );

    // Should have emitted a MessageInjected event
    let mut saw_injected = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, Event::MessageInjected { .. }) {
            saw_injected = true;
        }
    }
    assert!(saw_injected, "expected MessageInjected event from steering");
}
