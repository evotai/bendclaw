//! Tests for ToolStack::build — verifies the assembly boundary:
//! - ToolStackConfig with allowed_tool_names filters dispatched calls

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::run::execution::skills::SkillExecutor;
use bendclaw::kernel::run::execution::skills::SkillOutput;
use bendclaw::kernel::run::execution::tools::ToolStack;
use bendclaw::kernel::run::execution::tools::ToolStackConfig;
use bendclaw::kernel::run::execution::tools::TurnContext;
use bendclaw::kernel::tools::definition::tool_registry::ToolRegistry;
use bendclaw::kernel::tools::run_labels::RunLabels;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::kernel::tools::ToolResult;
use bendclaw::kernel::tools::ToolRuntime;
use bendclaw::kernel::Impact;
use bendclaw::kernel::OpType;
use bendclaw::llm::message::ToolCall;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// ── Mocks ────────────────────────────────────────────────────────────────

struct MockTool {
    name: String,
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
        Ok(ToolResult::ok(format!("{} output", self.name)))
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

// ── Helpers ──────────────────────────────────────────────────────────────

fn build_tool_stack(
    tools: Vec<Arc<dyn Tool>>,
    allowed: Option<HashSet<String>>,
) -> (ToolStack, mpsc::Receiver<Event>) {
    let cancel = CancellationToken::new();
    let mut registry = ToolRegistry::new();
    for t in tools {
        registry.register(t);
    }
    let (tx, rx) = mpsc::channel(128);
    let workspace = bendclaw_test_harness::mocks::context::test_workspace(
        std::env::temp_dir().join("bendclaw-test-tool-stack"),
    );
    let labels = Arc::new(RunLabels {
        trace_id: "t1".into(),
        run_id: "r1".into(),
        session_id: "s1".into(),
        agent_id: "a1".into(),
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
    let stack = ToolStack::build(ToolStackConfig {
        toolset: bendclaw::kernel::tools::definition::toolset::Toolset {
            definitions: Arc::new(definitions),
            bindings: Arc::new(bindings),
            tools: Arc::new(tools_schema),
            allowed_tool_names: allowed,
        },
        skill_executor: Arc::new(NoopSkillExecutor),
        tool_context: ToolContext {
            user_id: "u1".into(),
            session_id: "s1".into(),
            agent_id: "a1".into(),
            run_id: "r1".into(),
            trace_id: "t1".into(),
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
        cancel,
        trace: bendclaw::kernel::trace::Trace::new(bendclaw::kernel::trace::TraceRecorder::noop(
            "t1", "r1", "a1", "s1", "u1",
        )),
        event_tx: tx,
    });
    (stack, rx)
}

fn tc() -> TurnContext {
    TurnContext {
        turn: 1,
        loop_span_id: "loop-1".into(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn tool_stack_allowed_filter_blocks_unlisted_tool() {
    let mut allowed = HashSet::new();
    allowed.insert("read".to_string());

    let (mut stack, mut rx) = build_tool_stack(
        vec![
            Arc::new(MockTool {
                name: "read".into(),
            }),
            Arc::new(MockTool {
                name: "shell".into(),
            }),
        ],
        Some(allowed),
    );

    // Dispatch both tools — only file_read is allowed
    let calls = vec![
        ToolCall {
            id: "tc1".into(),
            name: "read".into(),
            arguments: "{}".into(),
        },
        ToolCall {
            id: "tc2".into(),
            name: "shell".into(),
            arguments: "{}".into(),
        },
    ];
    let deadline = Instant::now() + Duration::from_secs(5);
    let output = stack.orchestrator.dispatch(&calls, deadline, tc()).await;

    assert_eq!(output.invoked_names, vec!["read", "shell"]);

    // Collect events — shell should have success=false (filtered)
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    let end_events: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::ToolEnd { name, success, .. } => Some((name.clone(), *success)),
            _ => None,
        })
        .collect();

    // file_read should succeed, shell should fail (filtered out)
    let file_read_ok = end_events.iter().any(|(n, s)| n == "read" && *s);
    let shell_blocked = end_events.iter().any(|(n, s)| n == "shell" && !*s);
    assert!(file_read_ok, "file_read should succeed");
    assert!(
        shell_blocked,
        "shell should be blocked by allowed_tool_names filter"
    );
}

#[tokio::test]
async fn tool_stack_no_filter_allows_all_tools() {
    let (mut stack, _rx) = build_tool_stack(
        vec![
            Arc::new(MockTool {
                name: "read".into(),
            }),
            Arc::new(MockTool {
                name: "shell".into(),
            }),
        ],
        None,
    );

    let calls = vec![
        ToolCall {
            id: "tc1".into(),
            name: "read".into(),
            arguments: "{}".into(),
        },
        ToolCall {
            id: "tc2".into(),
            name: "shell".into(),
            arguments: "{}".into(),
        },
    ];
    let deadline = Instant::now() + Duration::from_secs(5);
    let output = stack.orchestrator.dispatch(&calls, deadline, tc()).await;

    // Both tools should produce result messages (started + completed + result per tool)
    assert_eq!(output.invoked_names.len(), 2);
    // All messages should contain output (no error for either tool)
    let has_error = output
        .messages
        .iter()
        .any(|m| m.text().contains("not available"));
    assert!(
        !has_error,
        "no tool should be filtered when allowed_tool_names is None"
    );
}
