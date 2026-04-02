use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bendclaw::base::ErrorCode;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::kernel::session::core::session_state::SessionState;
use bendclaw::kernel::session::runtime::session_resources::SessionResources;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::session::SessionManager;
use bendclaw::kernel::skills::sync::SkillIndex;
use bendclaw::kernel::tools::definition::toolset::Toolset;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::tool::ToolSchema;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;
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
    let projector = Arc::new(SkillIndex::new(
        workspace_dir,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ));
    let config = Arc::new(AgentConfig::default());
    let meta_pool = pool.with_database("evotai_meta").expect("meta pool");
    let org = Arc::new(OrgServices::new(meta_pool, projector, &config, llm.clone()));
    Arc::new(Session::new(
        session_id.into(),
        agent_id.into(),
        "u1".into(),
        SessionResources {
            workspace,
            toolset: Toolset {
                definitions: Arc::new(vec![]),
                bindings: Arc::new(std::collections::HashMap::new()),
                tools: Arc::new(vec![]),
                allowed_tool_names: None,
            },
            org,
            store: Arc::new(
                bendclaw::kernel::session::store::json::JsonSessionStore::new(
                    std::path::PathBuf::from("/tmp/test-store"),
                ),
            ),
            trace_factory: Arc::new(bendclaw::kernel::trace::factory::NoopTraceFactory),
            llm: Arc::new(RwLock::new(llm)),
            config,
            prompt_variables: vec![],
            cluster_client: None,
            directive: None,
            trace_writer: bendclaw::kernel::trace::TraceWriter::noop(),
            persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
            prompt_config: None,
            before_turn_hook: None,
            steering_source: None,
            prompt_resolver: std::sync::Arc::new(
                bendclaw::kernel::run::planning::LocalPromptResolver::new(
                    bendclaw::kernel::run::planning::PromptSeed::default(),
                    std::sync::Arc::new(vec![]),
                    std::path::PathBuf::from("/tmp"),
                ),
            ),
            context_provider: std::sync::Arc::new(
                bendclaw::kernel::session::backend::noop::NoopBackend,
            ),
            run_initializer: std::sync::Arc::new(
                bendclaw::kernel::session::backend::noop::NoopBackend,
            ),
            skill_executor: std::sync::Arc::new(
                bendclaw::kernel::run::execution::skills::NoopSkillExecutor,
            ),
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
