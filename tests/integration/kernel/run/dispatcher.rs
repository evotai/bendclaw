use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use bendclaw::kernel::run::dispatcher::DispatchKind;
use bendclaw::kernel::run::dispatcher::ToolCallResult;
use bendclaw::kernel::run::dispatcher::ToolDispatcher;
use bendclaw::kernel::skills::executor::SkillExecutor;
use bendclaw::kernel::skills::executor::SkillOutput;
use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::kernel::tools::ToolResult;
use bendclaw::kernel::ErrorCode;
use bendclaw::kernel::Impact;
use bendclaw::kernel::OpType;
use bendclaw::llm::message::ToolCall;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
enum MockToolBehavior {
    Ok(String),
    ToolError(String),
    Sleep(Duration),
}

struct MockTool {
    name: String,
    behavior: MockToolBehavior,
}

impl OperationClassifier for MockTool {
    fn op_type(&self) -> OpType {
        OpType::MemoryRead
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
    ) -> bendclaw::base::Result<ToolResult> {
        match &self.behavior {
            MockToolBehavior::Ok(out) => Ok(ToolResult::ok(out.clone())),
            MockToolBehavior::ToolError(msg) => Ok(ToolResult::error(msg.clone())),
            MockToolBehavior::Sleep(d) => {
                tokio::time::sleep(*d).await;
                Ok(ToolResult::ok("late"))
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
    ) -> bendclaw::base::Result<SkillOutput> {
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

fn dispatcher(
    tools: Vec<Arc<dyn Tool>>,
    skill_executor: Arc<dyn SkillExecutor>,
    cancel: CancellationToken,
) -> ToolDispatcher {
    let mut registry = ToolRegistry::new();
    for t in tools {
        registry.register(t);
    }
    ToolDispatcher::new(
        Arc::new(registry),
        skill_executor,
        ToolContext {
            user_id: "u1".into(),
            session_id: "s1".into(),
            agent_id: "a1".into(),
            workspace: bendclaw_test_harness::mocks::context::test_workspace(
                std::env::temp_dir().join("bendclaw-test-dispatcher"),
            ),
            pool: bendclaw_test_harness::mocks::context::dummy_pool(),
        },
        cancel,
    )
}

#[test]
fn parse_calls_marks_tool_vs_skill_and_handles_bad_json() {
    let d = dispatcher(
        vec![Arc::new(MockTool {
            name: "memory_read".to_string(),
            behavior: MockToolBehavior::Ok("ok".to_string()),
        })],
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

    let parsed = d.parse_calls(&calls);
    assert_eq!(parsed.len(), 2);
    assert!(matches!(parsed[0].kind, DispatchKind::Tool));
    assert!(matches!(parsed[1].kind, DispatchKind::Skill));
    assert_eq!(parsed[0].arguments["key"], "k");
    assert!(parsed[1].arguments.is_object());
    assert!(parsed[1]
        .arguments
        .as_object()
        .is_some_and(|o| o.is_empty()));
}

#[test]
fn parse_calls_classifies_tool_vs_skill_and_parses_arguments() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(MockTool {
        name: "memory_read".to_string(),
        behavior: MockToolBehavior::Ok("ok".to_string()),
    }));

    let d = ToolDispatcher::new(
        Arc::new(registry),
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".into(),
        ))),
        ToolContext {
            user_id: "u".into(),
            session_id: "s".into(),
            agent_id: "a".into(),
            workspace: bendclaw_test_harness::mocks::context::test_workspace(
                std::env::temp_dir().join("bendclaw-test-dispatcher2"),
            ),
            pool: bendclaw_test_harness::mocks::context::dummy_pool(),
        },
        CancellationToken::new(),
    );

    let calls = vec![
        ToolCall {
            id: "tc1".into(),
            name: "memory_read".into(),
            arguments: r#"{"a":1}"#.into(),
        },
        ToolCall {
            id: "tc2".into(),
            name: "custom_skill".into(),
            arguments: "not-json".into(),
        },
    ];
    let parsed = d.parse_calls(&calls);

    assert!(matches!(parsed[0].kind, DispatchKind::Tool));
    assert_eq!(parsed[0].arguments["a"], 1);
    assert!(matches!(parsed[1].kind, DispatchKind::Skill));
    assert!(parsed[1].arguments.is_object());
    assert!(parsed[1]
        .arguments
        .as_object()
        .is_some_and(|o| o.is_empty()));
}

#[tokio::test]
async fn execute_calls_handles_tool_success_and_tool_error() -> Result<()> {
    let d = dispatcher(
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

    let parsed = d.parse_calls(&calls);
    let outcomes = d
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;
    assert_eq!(outcomes.len(), 2);

    match &outcomes[0].result {
        ToolCallResult::Success(out, meta) => {
            assert_eq!(out, "done");
            assert_eq!(meta.op_type, OpType::MemoryRead);
        }
        _ => anyhow::bail!("expected success"),
    }
    match &outcomes[1].result {
        ToolCallResult::ToolError(msg, meta) => {
            assert_eq!(msg, "bad args");
            assert_eq!(meta.op_type, OpType::MemoryRead);
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
    let d = dispatcher(vec![], skill_exec.clone(), CancellationToken::new());

    let calls = vec![ToolCall {
        id: "tc1".into(),
        name: "run_skill".into(),
        arguments: r#"{"q":"hello"}"#.into(),
    }];

    let parsed = d.parse_calls(&calls);
    let outcomes = d
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

    let d_tool_err = dispatcher(
        vec![],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::ToolError(
            "skill rejected".to_string(),
        ))),
        CancellationToken::new(),
    );
    let out = d_tool_err
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;
    match &out[0].result {
        ToolCallResult::ToolError(msg, _) => assert_eq!(msg, "skill rejected"),
        _ => anyhow::bail!("expected skill tool error"),
    }

    let d_infra_err = dispatcher(
        vec![],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::InfraError(
            "executor down".to_string(),
        ))),
        CancellationToken::new(),
    );
    let out = d_infra_err
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

    let d_timeout = dispatcher(
        vec![Arc::new(MockTool {
            name: "slow_tool".to_string(),
            behavior: MockToolBehavior::Sleep(Duration::from_millis(30)),
        })],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        CancellationToken::new(),
    );
    let parsed = d_timeout.parse_calls(&calls);
    let out = d_timeout
        .execute_calls(&parsed, Instant::now() + Duration::from_millis(5))
        .await;
    match &out[0].result {
        ToolCallResult::InfraError(msg, _) => assert!(msg.contains("timed out")),
        _ => anyhow::bail!("expected timeout infra error"),
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    let d_cancel = dispatcher(
        vec![Arc::new(MockTool {
            name: "slow_tool".to_string(),
            behavior: MockToolBehavior::Sleep(Duration::from_millis(30)),
        })],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        cancel,
    );
    let parsed = d_cancel.parse_calls(&calls);
    let out = d_cancel
        .execute_calls(&parsed, Instant::now() + Duration::from_secs(2))
        .await;
    match &out[0].result {
        ToolCallResult::InfraError(msg, _) => assert_eq!(msg, "cancelled"),
        _ => anyhow::bail!("expected cancellation infra error"),
    }

    Ok(())
}

#[test]
fn memory_tool_schemas_filters_by_ids() {
    let d = dispatcher(
        vec![
            Arc::new(MockTool {
                name: "memory_read".to_string(),
                behavior: MockToolBehavior::Ok("ok".to_string()),
            }),
            Arc::new(MockTool {
                name: "memory_search".to_string(),
                behavior: MockToolBehavior::Ok("ok".to_string()),
            }),
            Arc::new(MockTool {
                name: "other_tool".to_string(),
                behavior: MockToolBehavior::Ok("ok".to_string()),
            }),
        ],
        Arc::new(MockSkillExecutor::new(MockSkillBehavior::OkString(
            "ok".to_string(),
        ))),
        CancellationToken::new(),
    );

    let schemas = d.memory_tool_schemas(&[
        bendclaw::kernel::tools::ToolId::MemoryRead,
        bendclaw::kernel::tools::ToolId::MemorySearch,
    ]);
    assert_eq!(schemas.len(), 2);
    let names: Vec<_> = schemas.iter().map(|s| s.function.name.as_str()).collect();
    assert!(names.contains(&"memory_read"));
    assert!(names.contains(&"memory_search"));
}
