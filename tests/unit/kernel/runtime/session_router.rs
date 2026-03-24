use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bendclaw::base::ErrorCode;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::pending_decision::DecisionOption;
use bendclaw::kernel::runtime::pending_decision::PendingDecision;
use bendclaw::kernel::runtime::turn_relation::RunSnapshot;
use bendclaw::kernel::runtime::turn_relation::TurnRelation;
use bendclaw::kernel::runtime::turn_relation::TurnRelationClassifier;
use bendclaw::kernel::runtime::SubmitResult;
use bendclaw::kernel::session::session::SessionState;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::session::SessionResources;
use bendclaw::kernel::skills::store::SkillStore;
use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::tool::ToolSchema;
use bendclaw::storage::AgentDatabases;
use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use crate::common::fake_databend::FakeDatabend;
use crate::common::test_runtime::test_runtime;

struct NoopLLM;

#[async_trait]
impl LLMProvider for NoopLLM {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> bendclaw::base::Result<LLMResponse> {
        Err(ErrorCode::internal("noop llm"))
    }

    fn chat_stream(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> ResponseStream {
        let (_tx, stream) = ResponseStream::channel(1);
        stream
    }
}

fn make_session(session_id: &str, agent_id: &str) -> Arc<Session> {
    let llm: Arc<dyn LLMProvider> = Arc::new(NoopLLM);
    let workspace_dir =
        std::env::temp_dir().join(format!("bendclaw-router-test-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&workspace_dir);
    let workspace = Arc::new(Workspace::new(
        workspace_dir.clone(),
        workspace_dir.clone(),
        vec!["PATH".into(), "HOME".into()],
        std::collections::HashMap::new(),
        Duration::from_secs(5),
        Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    ));
    let fake = FakeDatabend::new(|_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    let pool = fake.pool();
    let databases = Arc::new(AgentDatabases::new(pool.clone(), "unit_").unwrap());
    let skills = Arc::new(SkillStore::new(databases, workspace_dir, None));
    Arc::new(Session::new(
        session_id.into(),
        agent_id.into(),
        "u1".into(),
        SessionResources {
            workspace,
            tool_registry: Arc::new(ToolRegistry::new()),
            skills,
            tools: Arc::new(vec![]),
            storage: Arc::new(AgentStore::new(pool, llm.clone())),
            llm: Arc::new(RwLock::new(llm)),
            config: Arc::new(AgentConfig::default()),
            variables: vec![],
            recall: None,
            cluster_client: None,
            directive: None,
            trace_writer: bendclaw::kernel::trace::TraceWriter::noop(),
            persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
            cached_config: None,
        },
    ))
}

fn set_running(session: &Arc<Session>, run_id: &str) {
    *session.state.lock() = SessionState::Running {
        run_id: run_id.into(),
        cancel: CancellationToken::new(),
        started_at: std::time::Instant::now(),
        iteration: Arc::new(AtomicU32::new(0)),
        inbox_tx: tokio::sync::mpsc::channel(1).0,
        event_inject_tx: tokio::sync::mpsc::channel(1).0,
    };
}

// ── cancel / status commands ──────────────────────────────────────────────────

#[tokio::test]
async fn cancel_command_returns_control() {
    let fake = FakeDatabend::new(|_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    let runtime = test_runtime(fake);
    let result = runtime
        .submit_turn("a1", "s1", "u1", "cancel", "t1", None, "", "", false)
        .await
        .unwrap();
    assert!(matches!(result, SubmitResult::Control { .. }));
}

#[tokio::test]
async fn status_command_returns_control() {
    let fake = FakeDatabend::new(|_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    let runtime = test_runtime(fake);
    let result = runtime
        .submit_turn("a1", "s1", "u1", "status", "t1", None, "", "", false)
        .await
        .unwrap();
    assert!(matches!(result, SubmitResult::Control { .. }));
}

// ── running session + StubClassifier (ForkOrAsk) ─────────────────────────────

#[tokio::test]
async fn running_session_stub_classifier_returns_control_with_question() {
    let fake = FakeDatabend::new(|_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    let runtime = test_runtime(fake);

    let session = make_session("s1", "a1");
    set_running(&session, "r1");
    runtime.sessions().insert(session);

    runtime.turn_coordinator().store_snapshot(
        "s1",
        RunSnapshot::from_input("s1", "r1", "clean test_ databases"),
    );

    let result = runtime
        .submit_turn(
            "a1",
            "s1",
            "u1",
            "also check warehouse slowness",
            "t1",
            None,
            "",
            "",
            false,
        )
        .await
        .unwrap();

    match result {
        SubmitResult::Control { message } => {
            assert!(message.contains("continue") || message.contains("switch"));
        }
        other => panic!("expected Control, got {other:?}"),
    }
}

// ── pending decision resolution ───────────────────────────────────────────────

#[tokio::test]
async fn pending_decision_continue_queues_followup() {
    let fake = FakeDatabend::new(|_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    let runtime = test_runtime(fake);

    let session = make_session("s1", "a1");
    set_running(&session, "r1");
    runtime.sessions().insert(session);

    runtime.turn_coordinator().store_decision(PendingDecision {
        session_id: "s1".to_string(),
        active_run_id: "r1".to_string(),
        question_id: "q1".to_string(),
        question_text: "What do you want?".to_string(),
        candidate_input: "new task".to_string(),
        options: vec![
            DecisionOption::ContinueCurrent,
            DecisionOption::CancelAndSwitch,
            DecisionOption::AppendAsFollowup,
        ],
        created_at: std::time::Instant::now(),
    });

    let result = runtime
        .submit_turn("a1", "s1", "u1", "continue", "t1", None, "", "", false)
        .await
        .unwrap();

    assert!(matches!(result, SubmitResult::Queued));
    assert!(runtime.turn_coordinator().get_decision("s1").is_none());
}

// ── Append path with custom classifier ───────────────────────────────────────

struct AppendClassifier;

#[async_trait]
impl TurnRelationClassifier for AppendClassifier {
    async fn classify(
        &self,
        _llm: &Arc<dyn LLMProvider>,
        _model: &str,
        _snapshot: &RunSnapshot,
        _new_input: &str,
    ) -> TurnRelation {
        TurnRelation::Append
    }
}

#[tokio::test]
async fn running_session_append_classifier_queues_followup() {
    let fake = FakeDatabend::new(|_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    let runtime = test_runtime(fake);
    runtime
        .turn_coordinator()
        .set_classifier(Arc::new(AppendClassifier));

    let session = make_session("s1", "a1");
    set_running(&session, "r1");
    runtime.sessions().insert(session);

    runtime
        .turn_coordinator()
        .store_snapshot("s1", RunSnapshot::from_input("s1", "r1", "list databases"));

    let result = runtime
        .submit_turn(
            "a1",
            "s1",
            "u1",
            "also show sizes",
            "t1",
            None,
            "",
            "",
            false,
        )
        .await
        .unwrap();

    assert!(matches!(result, SubmitResult::Queued));
}
#[tokio::test]
async fn pending_decision_append_queues_followup() {
    let fake = FakeDatabend::new(|_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    let runtime = test_runtime(fake);

    let session = make_session("s1", "a1");
    set_running(&session, "r1");
    runtime.sessions().insert(session);

    runtime.turn_coordinator().store_decision(PendingDecision {
        session_id: "s1".to_string(),
        active_run_id: "r1".to_string(),
        question_id: "q1".to_string(),
        question_text: "What do you want?".to_string(),
        candidate_input: "new task".to_string(),
        options: vec![
            DecisionOption::ContinueCurrent,
            DecisionOption::CancelAndSwitch,
            DecisionOption::AppendAsFollowup,
        ],
        created_at: std::time::Instant::now(),
    });

    let result = runtime
        .submit_turn(
            "a1",
            "s1",
            "u1",
            "append it after",
            "t1",
            None,
            "",
            "",
            false,
        )
        .await
        .unwrap();

    assert!(matches!(result, SubmitResult::Queued));
    assert!(runtime.turn_coordinator().get_decision("s1").is_none());
}
