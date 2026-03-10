//! Test helpers for building Session.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::session::SessionResources;
use bendclaw::kernel::tools::registry::create_session_tools;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::storage::Pool;
use parking_lot::RwLock;

use crate::mocks::skill::MockSkillCatalog;
use crate::mocks::skill::MockSkillStoreFactory;

/// Build a test Workspace for a temp directory.
pub fn test_workspace(dir: std::path::PathBuf) -> Arc<Workspace> {
    Arc::new(Workspace::new(
        dir,
        vec!["PATH".into(), "HOME".into()],
        HashMap::new(),
        std::time::Duration::from_secs(5),
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
        workspace: test_workspace(dir),
        pool: dummy_pool(),
    }
}

pub async fn test_session(llm: Arc<dyn LLMProvider>) -> Result<Session> {
    let skills: Arc<dyn bendclaw::kernel::skills::catalog::SkillCatalog> =
        Arc::new(MockSkillCatalog::new());
    let config = Arc::new(AgentConfig::default());

    let pool = crate::common::setup::pool().await?;

    let storage = Arc::new(AgentStore::new(pool.clone(), llm.clone()));

    let workspace_dir = std::env::temp_dir().join("bendclaw-test-session");
    let _ = std::fs::create_dir_all(&workspace_dir);

    let workspace = test_workspace(workspace_dir);

    let channels = Arc::new(bendclaw::kernel::channel::registry::ChannelRegistry::new());
    let tool_registry = Arc::new(create_session_tools(
        storage.clone(),
        skills.clone(),
        Arc::new(MockSkillStoreFactory),
        pool.clone(),
        channels,
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
        },
    ))
}
