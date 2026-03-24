use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bendclaw::base::ErrorCode;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::session::SessionState;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::session::SessionManager;
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
        let (_writer, stream) = ResponseStream::channel(1);
        stream
    }
}

fn test_session(session_id: &str, agent_id: &str) -> Arc<Session> {
    let llm: Arc<dyn LLMProvider> = Arc::new(NoopLLM);
    let workspace_dir =
        std::env::temp_dir().join(format!("bendclaw-session-manager-{}", ulid::Ulid::new()));
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

#[test]
fn invalidate_by_agent_evicts_idle_and_marks_running_sessions_stale() {
    let manager = SessionManager::new();
    let idle = test_session("idle", "a1");
    let running = test_session("running", "a1");
    let other = test_session("other", "a2");
    *running.state.lock() = SessionState::Running {
        run_id: "r1".into(),
        cancel: CancellationToken::new(),
        started_at: std::time::Instant::now(),
        iteration: Arc::new(AtomicU32::new(1)),
        inbox_tx: tokio::sync::mpsc::channel(1).0,
    };

    manager.insert(idle.clone());
    manager.insert(running.clone());
    manager.insert(other.clone());

    let result = manager.invalidate_by_agent("a1");

    assert_eq!(result.evicted_idle, 1);
    assert_eq!(result.marked_running, 1);
    assert!(manager.get("idle").is_none());
    assert!(manager.get("running").is_some());
    assert!(running.is_stale());
    assert!(running.is_running());
    assert!(!other.is_stale());
}
