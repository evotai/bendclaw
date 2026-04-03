use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use bendclaw::execution::skills::SkillExecutor;
use bendclaw::execution::skills::SkillOutput;
use bendclaw::execution::tools::tool_executor::CallExecutor;
use bendclaw::execution::tools::tool_result::ToolCallResult;
use bendclaw::kernel::ErrorCode;
use bendclaw::llm::message::ToolCall;
use bendclaw::tools::definition::tool_definition::ToolDefinition;
use bendclaw::tools::selection::tool_registry::ToolRegistry;
use bendclaw::tools::Impact;
use bendclaw::tools::OpType;
use bendclaw::tools::OperationClassifier;
use bendclaw::tools::Tool;
use bendclaw::tools::ToolContext;
use bendclaw::tools::ToolResult;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
enum MockToolBehavior {
    Ok(String),
    ToolError(String),
    Pending,
}

struct MockTool {
    name: String,
    behavior: MockToolBehavior,
}

impl OperationClassifier for MockTool {
    fn op_type(&self) -> OpType {
        OpType::FileRead
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }

    fn summarize(&self, _args: &serde_json::Value) -> String {
        format!("{} summary", self.name)
    }
}
#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "mock tool"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    async fn execute_with_context(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> bendclaw::types::Result<ToolResult> {
        match &self.behavior {
            MockToolBehavior::Ok(out) => Ok(ToolResult::ok(out.clone())),
            MockToolBehavior::ToolError(msg) => Ok(ToolResult::error(msg.clone())),
            MockToolBehavior::Pending => {
                futures::future::pending::<()>().await;
                unreachable!("pending tool should never resolve")
            }
        }
    }
}

#[derive(Clone)]
enum MockSkillBehavior {
    OkString(String),
    OkJson(serde_json::Value),
    ToolError(String),
    InfraError(String),
}

struct MockSkillExecutor {
    behavior: MockSkillBehavior,
    seen_args: Mutex<Vec<String>>,
}

impl MockSkillExecutor {
    fn new(behavior: MockSkillBehavior) -> Self {
        Self {
            behavior,
            seen_args: Mutex::new(Vec::new()),
        }
    }

    fn seen_args(&self) -> Vec<String> {
        self.seen_args.lock().clone()
    }
}
#[async_trait]
impl SkillExecutor for MockSkillExecutor {
    async fn execute(
        &self,
        _skill_name: &str,
        args: &[String],
    ) -> bendclaw::types::Result<SkillOutput> {
        *self.seen_args.lock() = args.to_vec();
        match &self.behavior {
            MockSkillBehavior::OkString(s) => Ok(SkillOutput {
                data: Some(serde_json::Value::String(s.clone())),
                error: None,
            }),
            MockSkillBehavior::OkJson(v) => Ok(SkillOutput {
                data: Some(v.clone()),
                error: None,
            }),
            MockSkillBehavior::ToolError(msg) => Ok(SkillOutput {
                data: None,
                error: Some(msg.clone()),
            }),
            MockSkillBehavior::InfraError(msg) => Err(ErrorCode::internal(msg.clone())),
        }
    }
}

fn build_executor(
    tools: Vec<Arc<dyn Tool>>,
    skill_executor: Arc<dyn SkillExecutor>,
    cancel: CancellationToken,
) -> CallExecutor {
    build_executor_with_skills(tools, vec![], skill_executor, cancel)
}

fn build_executor_with_skills(
    tools: Vec<Arc<dyn Tool>>,
    skill_names: Vec<&str>,
    skill_executor: Arc<dyn SkillExecutor>,
    cancel: CancellationToken,
) -> CallExecutor {
    let mut registry = ToolRegistry::new();
    for t in &tools {
        registry.register(t.clone());
    }
    let mut definitions: Vec<ToolDefinition> = registry
        .iter_tools()
        .map(|t| ToolDefinition::from_builtin(t.as_ref()))
        .collect();
    let mut bindings: std::collections::HashMap<
        String,
        bendclaw::tools::definition::tool_target::ToolTarget,
    > = registry
        .iter_tools()
        .map(|t| {
            (
                t.name().to_string(),
                bendclaw::tools::definition::tool_target::ToolTarget::Builtin(t.clone()),
            )
        })
        .collect();
    for name in skill_names {
        definitions.push(ToolDefinition::from_skill(
            name.to_string(),
            format!("{name} skill"),
            serde_json::json!({"type": "object"}),
        ));
        bindings.insert(
            name.to_string(),
            bendclaw::tools::definition::tool_target::ToolTarget::Skill,
        );
    }
    let tools_schema: Vec<bendclaw::llm::tool::ToolSchema> =
        definitions.iter().map(|d| d.to_tool_schema()).collect();
    let toolset = bendclaw::tools::definition::toolset::Toolset {
        definitions: Arc::new(definitions),
        bindings: Arc::new(bindings),
        tools: Arc::new(tools_schema),
        allowed_tool_names: None,
    };
    CallExecutor::new(
        &toolset,
        skill_executor,
        ToolContext {
            user_id: "u1".into(),
            session_id: "s1".into(),
            agent_id: "a1".into(),
            run_id: "r1".into(),
            trace_id: "t1".into(),
            workspace: bendclaw_test_harness::mocks::context::test_workspace(
                std::env::temp_dir().join("bendclaw-test-dispatcher"),
            ),
            is_dispatched: false,
            runtime: bendclaw::tools::ToolRuntime {
                event_tx: None,
                cancel: cancel.clone(),
                tool_call_id: None,
            },
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
        },
        cancel,
        tokio::sync::mpsc::channel(1).0,
    )
}

#[test]
fn parse_calls_marks_tool_vs_skill_and_handles_bad_json() {
    let exec = build_executor_with_skills(
        vec![Arc::new(MockTool {
            name: "memory_read".to_string(),
            behavior: MockToolBehavior::Ok("ok".to_string()),
        })],
        vec!["custom_skill"],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        CancellationToken::new(),
    );

    let calls = vec![
        ToolCall {
            id: "tc1".into(),
            name: "memory_read".into(),
            arguments: r#"{"key":"k"}"#.into(),
        },
        ToolCall {
            id: "tc2".into(),
            name: "custom_skill".into(),
            arguments: "not-json".into(),
        },
    ];

    let parsed = exec.parse_calls(&calls);
    assert_eq!(parsed.len(), 2);
    assert!(parsed[0].is_builtin());
    assert!(parsed[1].is_skill());
    assert_eq!(parsed[0].arguments["key"], "k");
    assert!(parsed[1].arguments.is_object());
    assert!(parsed[1]
        .arguments
        .as_object()
        .is_some_and(|o| o.is_empty()));
}

#[tokio::test]
async fn execute_calls_handles_tool_success_anexec_tool_error() -> Result<()> {
    let exec = build_executor(
        vec![
            Arc::new(MockTool {
                name: "ok_tool".to_string(),
                behavior: MockToolBehavior::Ok("done".to_string()),
            }),
            Arc::new(MockTool {
                name: "err_tool".to_string(),
                behavior: MockToolBehavior::ToolError("bad args".to_string()),
            }),
        ],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        CancellationToken::new(),
    );

    let calls = vec![
        ToolCall {
            id: "tc1".into(),
            name: "ok_tool".into(),
            arguments: "{}".into(),
        },
        ToolCall {
            id: "tc2".into(),
            name: "err_tool".into(),
            arguments: "{}".into(),
        },
    ];

    let parsed = exec.parse_calls(&calls);
    let outcomes = exec
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;
    assert_eq!(outcomes.len(), 2);

    match &outcomes[0].result {
        ToolCallResult::Success(out, meta) => {
            assert_eq!(out, "done");
            assert_eq!(meta.op_type, OpType::FileRead);
        }
        _ => anyhow::bail!("expected success"),
    }
    match &outcomes[1].result {
        ToolCallResult::ToolError(msg, meta) => {
            assert_eq!(msg, "bad args");
            assert_eq!(meta.op_type, OpType::FileRead);
        }
        _ => anyhow::bail!("expected tool error"),
    }

    Ok(())
}

#[tokio::test]
async fn execute_calls_handles_skill_success_and_errors() -> Result<()> {
    let skill_exec = Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkJson(
        serde_json::json!({"ok": true}),
    )));
    let exec = build_executor_with_skills(
        vec![],
        vec!["run_skill"],
        skill_exec.clone(),
        CancellationToken::new(),
    );

    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "run_skill".into(),
        arguments: r#"{"q":"hello"}"#.into(),
    }];

    let parsed = exec.parse_calls(&calls);
    let outcomes = exec
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;

    match &outcomes[0].result {
        ToolCallResult::Success(out, meta) => {
            assert!(out.contains("\"ok\":true"));
            assert_eq!(meta.op_type, OpType::SkillRun);
        }
        _ => anyhow::bail!("expected skill success"),
    }

    let args = skill_exec.seen_args();
    assert!(args.contains(&"--q".to_string()));
    assert!(args.contains(&"hello".to_string()));

    let exec_tool_err = build_executor_with_skills(
        vec![],
        vec!["run_skill"],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::ToolError(
            "skill rejected".to_string(),
        ))),
        CancellationToken::new(),
    );
    let out = exec_tool_err
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;
    match &out[0].result {
        ToolCallResult::ToolError(msg, _) => assert_eq!(msg, "skill rejected"),
        _ => anyhow::bail!("expected skill tool error"),
    }

    let exec_infra_err = build_executor_with_skills(
        vec![],
        vec!["run_skill"],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::InfraError(
            "executor down".to_string(),
        ))),
        CancellationToken::new(),
    );
    let out = exec_infra_err
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;
    match &out[0].result {
        ToolCallResult::InfraError(msg, _) => assert!(msg.contains("executor down")),
        _ => anyhow::bail!("expected skill infra error"),
    }

    Ok(())
}

#[tokio::test]
async fn execute_calls_timeout_and_cancel_paths() -> Result<()> {
    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "slow_tool".into(),
        arguments: "{}".into(),
    }];

    let exec = build_executor(
        vec![Arc::new(MockTool {
            name: "slow_tool".to_string(),
            behavior: MockToolBehavior::Pending,
        })],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        CancellationToken::new(),
    );
    let parsed = exec.parse_calls(&calls);
    let out = exec
        .execute_calls(&parsed, Instant::now() + Duration::from_millis(1))
        .await;
    match &out[0].result {
        ToolCallResult::InfraError(msg, _) => assert!(msg.contains("timed out")),
        _ => anyhow::bail!("expected timeout infra error"),
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    let exec_cancel = build_executor(
        vec![Arc::new(MockTool {
            name: "slow_tool".to_string(),
            behavior: MockToolBehavior::Pending,
        })],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        cancel,
    );
    let parsed = exec_cancel.parse_calls(&calls);
    let out = exec_cancel
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;
    match &out[0].result {
        ToolCallResult::InfraError(msg, _) => assert_eq!(msg, "cancelled"),
        _ => anyhow::bail!("expected cancellation infra error"),
    }

    Ok(())
}

#[tokio::test]
async fn execute_calls_truncates_large_tool_output() -> Result<()> {
    let large_output = "x".repeat(300_000);
    let exec = build_executor(
        vec![Arc::new(MockTool {
            name: "big_tool".to_string(),
            behavior: MockToolBehavior::Ok(large_output),
        })],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        CancellationToken::new(),
    );

    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "big_tool".into(),
        arguments: "{}".into(),
    }];

    let parsed = exec.parse_calls(&calls);
    let outcomes = exec
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;

    match &outcomes[0].result {
        ToolCallResult::Success(out, _) => {
            assert!(out.len() < 300_000);
            assert!(out.contains("[truncated:"));
        }
        _ => anyhow::bail!("expected success"),
    }
    Ok(())
}

#[tokio::test]
async fn execute_calls_truncates_large_tool_error() -> Result<()> {
    let large_error = "e".repeat(300_000);
    let exec = build_executor(
        vec![Arc::new(MockTool {
            name: "err_tool".to_string(),
            behavior: MockToolBehavior::ToolError(large_error),
        })],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        CancellationToken::new(),
    );

    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "err_tool".into(),
        arguments: "{}".into(),
    }];

    let parsed = exec.parse_calls(&calls);
    let outcomes = exec
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;

    match &outcomes[0].result {
        ToolCallResult::ToolError(msg, _) => {
            assert!(msg.len() < 300_000);
            assert!(msg.contains("[truncated:"));
        }
        _ => anyhow::bail!("expected tool error"),
    }
    Ok(())
}

#[tokio::test]
async fn execute_calls_truncates_large_skill_output() -> Result<()> {
    let large_output = "s".repeat(300_000);
    let exec = build_executor_with_skills(
        vec![],
        vec!["big_skill"],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            large_output,
        ))),
        CancellationToken::new(),
    );

    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "big_skill".into(),
        arguments: "{}".into(),
    }];

    let parsed = exec.parse_calls(&calls);
    let outcomes = exec
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;

    match &outcomes[0].result {
        ToolCallResult::Success(out, _) => {
            assert!(out.len() < 300_000);
            assert!(out.contains("[truncated:"));
        }
        _ => anyhow::bail!("expected success"),
    }
    Ok(())
}

#[tokio::test]
async fn execute_calls_runs_tools_in_parallel() -> Result<()> {
    // Each tool sleeps 100ms. If run sequentially 3×100ms = 300ms.
    // If parallel, total should be ~100ms (well under 250ms).
    struct SlowTool {
        name: String,
        delay: Duration,
    }

    impl OperationClassifier for SlowTool {
        fn op_type(&self) -> OpType {
            OpType::FileRead
        }
        fn classify_impact(&self, _: &serde_json::Value) -> Option<Impact> {
            None
        }
        fn summarize(&self, _: &serde_json::Value) -> String {
            self.name.clone()
        }
    }

    #[async_trait]
    impl Tool for SlowTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "slow"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute_with_context(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
        ) -> bendclaw::types::Result<ToolResult> {
            tokio::time::sleep(self.delay).await;
            Ok(ToolResult::ok(format!("{} done", self.name)))
        }
    }

    let tools: Vec<Arc<dyn Tool>> = (0..3)
        .map(|i| {
            Arc::new(SlowTool {
                name: format!("slow_{i}"),
                delay: Duration::from_millis(100),
            }) as Arc<dyn Tool>
        })
        .collect();

    let exec = build_executor(
        tools,
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        CancellationToken::new(),
    );

    let calls: Vec<ToolCall> = (0..3)
        .map(|i| ToolCall {
            id: format!("tc{i}"),
            name: format!("slow_{i}"),
            arguments: "{}".into(),
        })
        .collect();

    let parsed = exec.parse_calls(&calls);
    let start = Instant::now();
    let outcomes = exec
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(5))
        .await;
    let elapsed = start.elapsed();

    assert_eq!(outcomes.len(), 3);
    for outcome in &outcomes {
        assert!(
            matches!(outcome.result, ToolCallResult::Success(..)),
            "all tools should succeed"
        );
    }
    // If truly parallel, 3×100ms tools should complete well under 250ms
    assert!(
        elapsed < Duration::from_millis(250),
        "expected parallel execution under 250ms, got {:?}",
        elapsed
    );

    Ok(())
}
