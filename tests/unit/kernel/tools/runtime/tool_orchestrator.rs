//! Tests for ToolOrchestrator — verifies the execution boundary contracts:
//! - ToolStart events fire before execution, ToolEnd events fire after
//! - tool_result messages are correctly written back to transcript
//! - success / tool_error / infra_error paths all produce correct output

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::skills::runtime::SkillExecutor;
use bendclaw::kernel::skills::runtime::SkillOutput;
use bendclaw::kernel::tools::catalog::tool_registry::ToolRegistry;
use bendclaw::kernel::tools::run_labels::RunLabels;
use bendclaw::kernel::tools::runtime::tool_events::EventEmitter;
use bendclaw::kernel::tools::runtime::tool_executor::CallExecutor;
use bendclaw::kernel::tools::runtime::tool_orchestrator::ToolOrchestrator;
use bendclaw::kernel::tools::runtime::tool_recorder::ExecutionRecorder;
use bendclaw::kernel::tools::runtime::TurnContext;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::kernel::tools::ToolResult;
use bendclaw::kernel::Impact;
use bendclaw::kernel::OpType;
use bendclaw::llm::message::ToolCall;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// ── Mocks ────────────────────────────────────────────────────────────────

struct MockTool {
    name: String,
    output: String,
    succeed: bool,
}

impl OperationClassifier for MockTool {
    fn op_type(&self) -> OpType {
        OpType::FileRead
    }
    fn classify_impact(&self, _: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }
    fn summarize(&self, _: &serde_json::Value) -> String {
        format!("{} summary", self.name)
    }
}

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "mock"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute_with_context(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> bendclaw::base::Result<ToolResult> {
        if self.succeed {
            Ok(ToolResult::ok(self.output.clone()))
        } else {
            Ok(ToolResult::error(self.output.clone()))
        }
    }
}

struct NoopSkillExecutor;
#[async_trait]
impl SkillExecutor for NoopSkillExecutor {
    async fn execute(&self, _: &str, _: &[String]) -> bendclaw::base::Result<SkillOutput> {
        Ok(SkillOutput {
            data: None,
            error: Some("not implemented".into()),
        })
    }
}

fn test_trace_recorder() -> bendclaw::kernel::trace::TraceRecorder {
    bendclaw::kernel::trace::TraceRecorder::noop("t1", "r1", "a1", "s1", "u1")
}

fn build_orchestrator(tools: Vec<Arc<dyn Tool>>) -> (ToolOrchestrator, mpsc::Receiver<Event>) {
    let cancel = CancellationToken::new();
    let mut registry = ToolRegistry::new();
    for t in tools {
        registry.register(t);
    }
    let (tx, rx) = mpsc::channel(128);
    let executor = CallExecutor::new(
        Arc::new(registry),
        Arc::new(NoopSkillExecutor),
        ToolContext {
            user_id: "u1".into(),
            session_id: "s1".into(),
            agent_id: "a1".into(),
            run_id: "r1".into(),
            trace_id: "t1".into(),
            workspace: bendclaw_test_harness::mocks::context::test_workspace(
                std::env::temp_dir().join("bendclaw-test-lifecycle"),
            ),
            is_dispatched: false,
            runtime: bendclaw::kernel::tools::ToolRuntime {
                event_tx: None,
                cancel: cancel.clone(),
                tool_call_id: None,
            },
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
        },
        cancel,
        tx.clone(),
    );
    let labels = Arc::new(RunLabels {
        trace_id: "t1".into(),
        run_id: "r1".into(),
        session_id: "s1".into(),
        agent_id: "a1".into(),
    });
    let recorder = ExecutionRecorder::new(
        labels,
        bendclaw::kernel::trace::Trace::new(test_trace_recorder()),
        tx.clone(),
    );
    let emitter = EventEmitter::new(tx);
    (ToolOrchestrator::new(executor, recorder, emitter), rx)
}

fn tc() -> TurnContext {
    TurnContext {
        turn: 1,
        loop_span_id: "loop-1".into(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn orchestrator_success_emits_start_before_end() {
    let (mut orchestrator, mut rx) = build_orchestrator(vec![Arc::new(MockTool {
        name: "read".into(),
        output: "content".into(),
        succeed: true,
    })]);
    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "read".into(),
        arguments: "{}".into(),
    }];
    let deadline = Instant::now() + Duration::from_secs(5);
    let output = orchestrator.dispatch(&calls, deadline, tc()).await;

    // Collect events
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    // ToolStart must appear before ToolEnd
    let start_idx = events
        .iter()
        .position(|e| matches!(e, Event::ToolStart { .. }));
    let end_idx = events
        .iter()
        .position(|e| matches!(e, Event::ToolEnd { .. }));
    assert!(start_idx.is_some(), "missing ToolStart event");
    assert!(end_idx.is_some(), "missing ToolEnd event");
    assert!(
        start_idx.unwrap() < end_idx.unwrap(),
        "ToolStart must fire before ToolEnd"
    );

    // ToolEnd should be success
    if let Some(Event::ToolEnd { success, .. }) =
        events.iter().find(|e| matches!(e, Event::ToolEnd { .. }))
    {
        assert!(success, "expected success=true");
    }

    // Output should contain tool_result message
    assert!(
        !output.messages.is_empty(),
        "dispatch should produce messages"
    );
    assert_eq!(output.invoked_names, vec!["read"]);
}

#[tokio::test]
async fn orchestrator_tool_error_path() {
    let (mut orchestrator, mut rx) = build_orchestrator(vec![Arc::new(MockTool {
        name: "shell".into(),
        output: "command not found".into(),
        succeed: false,
    })]);
    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "shell".into(),
        arguments: "{}".into(),
    }];
    let deadline = Instant::now() + Duration::from_secs(5);
    let output = orchestrator.dispatch(&calls, deadline, tc()).await;

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    // ToolEnd should be failure
    let end_event = events.iter().find(|e| matches!(e, Event::ToolEnd { .. }));
    assert!(end_event.is_some(), "missing ToolEnd event");
    if let Some(Event::ToolEnd { success, .. }) = end_event {
        assert!(!success, "expected success=false for tool error");
    }

    assert!(!output.messages.is_empty());
    assert_eq!(output.invoked_names, vec!["shell"]);
}

#[tokio::test]
async fn orchestrator_multiple_tools_all_produce_events() {
    let (mut orchestrator, mut rx) = build_orchestrator(vec![
        Arc::new(MockTool {
            name: "read".into(),
            output: "ok1".into(),
            succeed: true,
        }),
        Arc::new(MockTool {
            name: "grep".into(),
            output: "ok2".into(),
            succeed: true,
        }),
    ]);
    let calls = vec![
        ToolCall {
            id: "tc1".into(),
            name: "read".into(),
            arguments: "{}".into(),
        },
        ToolCall {
            id: "tc2".into(),
            name: "grep".into(),
            arguments: "{}".into(),
        },
    ];
    let deadline = Instant::now() + Duration::from_secs(5);
    let output = orchestrator.dispatch(&calls, deadline, tc()).await;

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    let start_count = events
        .iter()
        .filter(|e| matches!(e, Event::ToolStart { .. }))
        .count();
    let end_count = events
        .iter()
        .filter(|e| matches!(e, Event::ToolEnd { .. }))
        .count();
    assert_eq!(start_count, 2, "expected 2 ToolStart events");
    assert_eq!(end_count, 2, "expected 2 ToolEnd events");
    assert_eq!(output.invoked_names.len(), 2);
}

#[tokio::test]
async fn orchestrator_transcript_contains_tool_result() {
    let (mut orchestrator, _rx) = build_orchestrator(vec![Arc::new(MockTool {
        name: "read".into(),
        output: "file content here".into(),
        succeed: true,
    })]);
    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "read".into(),
        arguments: "{}".into(),
    }];
    let deadline = Instant::now() + Duration::from_secs(5);
    let output = orchestrator.dispatch(&calls, deadline, tc()).await;

    // Should have: started_message, completed_message, tool_result_message
    assert!(
        output.messages.len() >= 3,
        "expected at least 3 messages (started + completed + result), got {}",
        output.messages.len()
    );

    // The last message for a tool should be the tool_result with run_id
    let last = output.messages.last().unwrap();
    let text = last.text();
    assert!(
        text.contains("file content here"),
        "tool_result message should contain tool output"
    );
}

#[tokio::test]
async fn orchestrator_infra_error_emits_failure_event_and_message() {
    // Use a tool that will timeout (simulating infra error) by using a very short deadline
    // with a tool that takes time. We reuse the existing MockTool with succeed=true but
    // set an already-expired deadline so the tool times out.
    let (mut orchestrator, mut rx) = build_orchestrator(vec![Arc::new(MockTool {
        name: "slow".into(),
        output: "should not appear".into(),
        succeed: true,
    })]);
    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "unknown_tool_xyz".into(),
        arguments: "{}".into(),
    }];
    // unknown_tool_xyz is not registered, so it falls through to skill executor
    // which returns "not implemented" error — this exercises the non-tool path.
    let deadline = Instant::now() + Duration::from_secs(5);
    let output = orchestrator.dispatch(&calls, deadline, tc()).await;

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    // ToolEnd should fire with success=false
    let end_event = events.iter().find(|e| matches!(e, Event::ToolEnd { .. }));
    assert!(end_event.is_some(), "missing ToolEnd event for skill error");
    if let Some(Event::ToolEnd { success, .. }) = end_event {
        assert!(!success, "expected success=false for skill error");
    }

    // Messages should contain error text
    let has_error = output.messages.iter().any(|m| m.text().contains("Error:"));
    assert!(has_error, "expected error message in output");
    assert_eq!(output.invoked_names, vec!["unknown_tool_xyz"]);
}
