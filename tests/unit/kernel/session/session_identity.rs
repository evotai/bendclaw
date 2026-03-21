use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::session::SessionResources;
use bendclaw::kernel::skills::store::SkillStore;
use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use parking_lot::RwLock;

#[tokio::test]
async fn session_belongs_to_matches_exact_agent_and_user() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let workspace_dir =
        std::env::temp_dir().join(format!("bendclaw-unit-session-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&workspace_dir);
    let workspace = Arc::new(Workspace::new(
        workspace_dir.clone(),
        workspace_dir.clone(),
        vec!["PATH".into(), "HOME".into()],
        std::collections::HashMap::new(),
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    ));
    let pool = bendclaw_test_harness::mocks::context::dummy_pool();
    let databases =
        Arc::new(bendclaw::storage::AgentDatabases::new(pool.clone(), "unit_").unwrap());
    let skills = Arc::new(SkillStore::new(databases, workspace_dir, None));
    let session = Session::new("s1".into(), "a1".into(), "u1".into(), SessionResources {
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
        trace_writer: bendclaw::kernel::trace::TraceWriter::spawn(),
        persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
        tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
        cached_config: None,
    });

    assert!(session.belongs_to("a1", "u1"));
    assert!(!session.belongs_to("a2", "u1"));
    assert!(!session.belongs_to("a1", "u2"));
    assert!(!session.belongs_to("a2", "u2"));
    Ok(())
}
