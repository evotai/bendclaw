//! Test helpers for building Session.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::recall::RecallStore;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::session::SessionResources;
use bendclaw::kernel::skills::remote::repository::DatabendSkillRepositoryFactory;
use bendclaw::kernel::tools::registry::create_session_tools;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::storage::Pool;
use parking_lot::RwLock;

use crate::mocks::skill::test_skill_store;
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
        pool: dummy_pool(),
        is_dispatched: false,
        runtime: bendclaw::kernel::tools::ToolRuntime {
            event_tx: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            cli_agent_state: bendclaw::kernel::tools::cli_agent::new_shared_state(),
            tool_call_id: None,
        },
        tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
    }
}

pub async fn test_session(llm: Arc<dyn LLMProvider>) -> Result<Session> {
    let config = Arc::new(AgentConfig::default());

    let pool = pool().await?;

    let databases =
        Arc::new(bendclaw::storage::AgentDatabases::new(pool.clone(), "test_").unwrap());

    let workspace_dir = std::env::temp_dir().join("bendclaw-test-session");
    let _ = std::fs::create_dir_all(&workspace_dir);

    let skills = test_skill_store(databases.clone(), workspace_dir.clone());

    let storage = Arc::new(AgentStore::new(pool.clone(), llm.clone()));

    let workspace = test_workspace(workspace_dir);

    let channels = Arc::new(bendclaw::kernel::channel::registry::ChannelRegistry::new());
    let skill_store_factory = Arc::new(DatabendSkillRepositoryFactory::new(databases));
    let recall_store = Arc::new(RecallStore::new(pool.clone()));
    let tool_registry = Arc::new(create_session_tools(
        storage.clone(),
        skills.clone(),
        skill_store_factory,
        pool.clone(),
        channels,
        "test-instance".to_string(),
        recall_store.clone(),
    ));

    let tools = Arc::new(tool_registry.tool_schemas());

    Ok(Session::new(
        "s1".to_string(),
        "a1".into(),
        "u1".into(),
        SessionResources {
            workspace,
            tool_registry,
            skills,
            tools,
            storage,
            llm: Arc::new(RwLock::new(llm)),
            config,
            variables: vec![],
            recall: Some(recall_store),
            cluster_client: None,
            directive: None,
            trace_writer: bendclaw::kernel::trace::TraceWriter::spawn(),
            persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
            cached_config: None,
        },
    ))
}
