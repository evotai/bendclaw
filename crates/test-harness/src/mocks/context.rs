//! Test helpers for building Session.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::kernel::session::runtime::session_resources::SessionResources;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::tools::tool_services::NoopSecretUsageSink;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::storage::Pool;
use parking_lot::RwLock;

use crate::mocks::skill::test_skill_projector;
use crate::setup::pool;

/// Build a test Workspace for a temp directory.
pub fn test_workspace(dir: std::path::PathBuf) -> Arc<Workspace> {
    Arc::new(Workspace::new(
        dir.clone(),
        dir,
        vec!["PATH".into(), "HOME".into()],
        HashMap::new(),
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    ))
}

/// Create a dummy Pool that points to a non-existent endpoint.
/// Suitable for tests that never actually query the database.
pub fn dummy_pool() -> Pool {
    Pool::new("http://localhost:0", "", "default").expect("dummy pool: invalid URL is unreachable")
}

/// Build a test `Session` with tools wired up.
pub fn test_tool_context() -> ToolContext {
    use ulid::Ulid;
    let dir = std::env::temp_dir().join(format!("bendclaw-test-ctx-{}", Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    ToolContext {
        user_id: format!("u-{}", Ulid::new()).into(),
        session_id: format!("s-{}", Ulid::new()).into(),
        agent_id: "a1".into(),
        run_id: "r-test".into(),
        trace_id: "t-test".into(),
        workspace: test_workspace(dir),
        is_dispatched: false,
        runtime: bendclaw::kernel::tools::ToolRuntime {
            event_tx: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            tool_call_id: None,
        },
        tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
    }
}

pub async fn test_session(llm: Arc<dyn LLMProvider>) -> Result<Session> {
    let config = Arc::new(AgentConfig::default());

    let pool = pool().await?;

    let _databases =
        Arc::new(bendclaw::storage::AgentDatabases::new(pool.clone(), "test_").unwrap());

    let workspace_dir = std::env::temp_dir().join("bendclaw-test-session");
    let _ = std::fs::create_dir_all(&workspace_dir);

    let catalog = test_skill_projector(workspace_dir.clone());

    let meta_pool = pool.with_database("evotai_meta")?;
    let org = Arc::new(OrgServices::new(
        meta_pool,
        catalog.clone(),
        &config,
        llm.clone(),
    ));

    let store: Arc<dyn bendclaw::kernel::session::store::SessionStore> = Arc::new(
        bendclaw::kernel::session::store::db::DbSessionStore::new(pool.clone()),
    );

    let workspace = test_workspace(workspace_dir);

    let channels = Arc::new(bendclaw::kernel::channel::registry::ChannelRegistry::new());
    let sink: Arc<dyn bendclaw::kernel::tools::tool_services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    let toolset = bendclaw::kernel::tools::selection::build_cloud_toolset(
        bendclaw::kernel::tools::selection::CloudToolsetDeps {
            org: org.clone(),
            databend_pool: pool.clone(),
            channels,
            node_id: "test-instance".to_string(),
            cluster: None,
            memory: None,
            secret_sink: sink,
            user_id: "test-user".to_string(),
        },
        None,
    );
    Ok(Session::new(
        "s1".to_string(),
        "a1".into(),
        "u1".into(),
        SessionResources {
            workspace,
            toolset,
            org,
            store: store,
            llm: Arc::new(RwLock::new(llm)),
            config,
            prompt_variables: vec![],
            cluster_client: None,
            directive: None,
            trace_writer: bendclaw::kernel::trace::TraceWriter::spawn(),
            trace_factory: Arc::new(bendclaw::kernel::trace::factory::NoopTraceFactory),
            persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
            prompt_config: None,
            before_turn_hook: None,
            steering_source: None,
            prompt_resolver: Arc::new(bendclaw::kernel::run::planning::LocalPromptResolver::new(
                bendclaw::kernel::run::planning::PromptSeed::default(),
                Arc::new(vec![]),
                std::path::PathBuf::from("/tmp"),
            )),
            context_provider: Arc::new(bendclaw::kernel::session::backend::noop::NoopBackend),
            run_initializer: Arc::new(bendclaw::kernel::session::backend::noop::NoopBackend),
            skill_executor: Arc::new(bendclaw::kernel::run::execution::skills::NoopSkillExecutor),
        },
    ))
}
