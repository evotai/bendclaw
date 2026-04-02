use std::collections::HashSet;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;

use bendclaw::kernel::run::context::Context;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::run::execution::compaction::Compactor;
use bendclaw::kernel::run::execution::llm::Engine;
use bendclaw::kernel::run::execution::tools::ToolStack;
use bendclaw::kernel::run::execution::tools::ToolStackConfig;
use bendclaw::kernel::run::planning::tool_view::ProgressiveToolView;
use bendclaw::kernel::run::result::Reason;
use bendclaw::kernel::tools::run_labels::RunLabels;
use bendclaw::kernel::tools::selection::tool_registry::ToolRegistry;
use bendclaw::kernel::tools::tool_id::ToolId;
use bendclaw::kernel::tools::tool_services::NoopSecretUsageSink;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::kernel::tools::ToolRuntime;
use bendclaw::kernel::trace::TraceRecorder;
use bendclaw::kernel::Message;
use bendclaw::storage::dal::trace::repo::SpanRepo;
use bendclaw::storage::dal::trace::repo::TraceRepo;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use bendclaw_test_harness::mocks::llm::MockTurn;
use tokio_util::sync::CancellationToken;

fn trace() -> TraceRecorder {
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

fn real_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    let sink: Arc<dyn bendclaw::kernel::tools::tool_services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    registry.register_builtin(
        ToolId::ListDir,
        Arc::new(bendclaw::kernel::tools::builtin::filesystem::ListDirTool),
    );
    registry.register_builtin(
        ToolId::Glob,
        Arc::new(bendclaw::kernel::tools::builtin::filesystem::GlobTool),
    );
    registry.register_builtin(
        ToolId::Bash,
        Arc::new(bendclaw::kernel::tools::builtin::shell::ShellTool::new(
            sink,
        )),
    );
    registry
}

fn build_engine_with_filter(
    llm: Arc<MockLLMProvider>,
    registry: ToolRegistry,
    allowed: Option<HashSet<String>>,
) -> (Engine, tokio::sync::mpsc::Receiver<Event>) {
    let cancel = CancellationToken::new();
    let (tx, rx) = Engine::create_channel();
    let (_inbox_tx, inbox_rx) = Engine::create_inbox();
    let workspace = bendclaw_test_harness::mocks::context::test_workspace(
        std::env::temp_dir().join("bendclaw-engine-smoke"),
    );
    let labels = Arc::new(RunLabels {
        trace_id: "trace-1".to_string(),
        run_id: "run-1".to_string(),
        session_id: "session-1".to_string(),
        agent_id: "agent-1".to_string(),
    });
    let definitions: Vec<bendclaw::kernel::tools::definition::tool_definition::ToolDefinition> =
        registry
            .iter_tools()
            .map(|t| {
                bendclaw::kernel::tools::definition::tool_definition::ToolDefinition::from_builtin(
                    t.as_ref(),
                )
            })
            .collect();
    let bindings: std::collections::HashMap<
        String,
        bendclaw::kernel::tools::definition::tool_target::ToolTarget,
    > = registry
        .iter_tools()
        .map(|t| {
            (
                t.name().to_string(),
                bendclaw::kernel::tools::definition::tool_target::ToolTarget::Builtin(t.clone()),
            )
        })
        .collect();
    let tools_schema: Vec<bendclaw::llm::tool::ToolSchema> =
        definitions.iter().map(|d| d.to_tool_schema()).collect();
    let tool_stack = ToolStack::build(ToolStackConfig {
        toolset: bendclaw::kernel::tools::definition::toolset::Toolset {
            definitions: Arc::new(definitions),
            bindings: Arc::new(bindings),
            tools: Arc::new(tools_schema),
            allowed_tool_names: allowed,
        },
        skill_executor: Arc::new(bendclaw::kernel::run::execution::skills::NoopSkillExecutor),
        tool_context: ToolContext {
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
        labels,
        cancel: cancel.clone(),
        trace: bendclaw::kernel::trace::Trace::new(trace()),
        event_tx: tx.clone(),
    });
    let ctx = Context {
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        session_id: "session-1".into(),
        run_id: "run-1".into(),
        turn: 1,
        trace_id: "trace-1".into(),
        llm: llm.clone(),
        model: "mock".into(),
        temperature: 0.0,
        max_iterations: 5,
        max_context_tokens: 250_000,
        max_duration: Duration::from_secs(30),
        tool_view: ProgressiveToolView::new(Arc::new(vec![])),
        system_prompt: "test".into(),
        messages: vec![Message::user("hello")],
    };
    let compactor = Compactor::new(llm, "mock".into(), cancel.clone());
    let engine = Engine::from_tx(
        ctx,
        tool_stack.orchestrator,
        compactor,
        cancel,
        Arc::new(AtomicU32::new(0)),
        trace(),
        tx,
        inbox_rx,
        None,
    );
    (engine, rx)
}

fn build_engine(llm: Arc<MockLLMProvider>) -> (Engine, tokio::sync::mpsc::Receiver<Event>) {
    build_engine_with_filter(llm, real_registry(), None)
}

#[tokio::test]
async fn engine_no_tool_call_completes_with_end_turn() {
    let llm = Arc::new(MockLLMProvider::with_text("done"));
    let (mut engine, _rx) = build_engine(llm);
    let result = engine.run().await.unwrap();
    assert_eq!(result.stop_reason, Reason::EndTurn);
    assert_eq!(result.iterations, 1);
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn engine_builtin_tool_dispatch_executes() {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "list_dir".to_string(),
            arguments: r#"{"path":"/tmp"}"#.to_string(),
        },
        MockTurn::Text("done".to_string()),
    ]));
    let (mut engine, _rx) = build_engine(llm);
    let result = engine.run().await.unwrap();
    assert_eq!(result.stop_reason, Reason::EndTurn);
    assert!(result.iterations >= 2);
    let tool_results: Vec<String> = result
        .messages
        .iter()
        .filter_map(|m| match m {
            Message::ToolResult { output, .. } => Some(output.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !tool_results.is_empty(),
        "should have tool result from list_dir"
    );
    assert!(
        !tool_results[0].contains("is not available in this session"),
        "tool result should be a real execution, not a filter error"
    );
}

#[tokio::test]
async fn engine_filter_blocks_disallowed_tool() {
    let allowed: HashSet<String> = ["list_dir"].iter().map(|s| s.to_string()).collect();
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "bash".to_string(),
            arguments: r#"{"command":"echo hi"}"#.to_string(),
        },
        MockTurn::Text("done".to_string()),
    ]));
    let (mut engine, _rx) = build_engine_with_filter(llm, real_registry(), Some(allowed));
    let result = engine.run().await.unwrap();
    assert_eq!(result.stop_reason, Reason::EndTurn);
    let tool_results: Vec<String> = result
        .messages
        .iter()
        .filter_map(|m| match m {
            Message::ToolResult { output, .. } => Some(output.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !tool_results.is_empty(),
        "should have tool result for blocked tool"
    );
    assert!(
        tool_results[0].contains("bash"),
        "error should mention the blocked tool name 'bash'"
    );
    assert!(
        tool_results[0].contains("not available"),
        "error should indicate tool is not available"
    );
}
