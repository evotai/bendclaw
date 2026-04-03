use std::sync::Arc;

use anyhow::Result;
use bendclaw::config::agent::AgentConfig;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::kernel::skills::sync::SkillIndex;
use bendclaw::kernel::tools::definition::toolset::Toolset;
use bendclaw::sessions::runtime::session_resources::SessionResources;
use bendclaw::sessions::workspace::SandboxResolver;
use bendclaw::sessions::workspace::Workspace;
use bendclaw::sessions::Session;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;
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
    let projector = Arc::new(SkillIndex::new(
        workspace_dir,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ));
    let config = Arc::new(AgentConfig::default());
    let meta_pool = pool.with_database("evotai_meta")?;
    let org = Arc::new(OrgServices::new(meta_pool, projector, &config, llm.clone()));
    let session = Session::new("s1".into(), "a1".into(), "u1".into(), SessionResources {
        workspace,
        toolset: Toolset {
            definitions: Arc::new(vec![]),
            bindings: Arc::new(std::collections::HashMap::new()),
            tools: Arc::new(vec![]),
            allowed_tool_names: None,
        },
        org,
        store: Arc::new(bendclaw::sessions::store::json::JsonSessionStore::new(
            std::path::PathBuf::from("/tmp/test-store"),
        )),
        trace_factory: Arc::new(bendclaw::kernel::trace::factory::NoopTraceFactory),
        llm: Arc::new(RwLock::new(llm)),
        config,
        prompt_variables: vec![],
        cluster_client: None,
        directive: None,
        trace_writer: bendclaw::kernel::trace::TraceWriter::spawn(),
        persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
        tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
        prompt_config: None,
        before_turn_hook: None,
        steering_source: None,
        prompt_resolver: std::sync::Arc::new(bendclaw::planning::LocalPromptResolver::new(
            bendclaw::planning::PromptSeed::default(),
            std::sync::Arc::new(vec![]),
            std::path::PathBuf::from("/tmp"),
        )),
        context_provider: std::sync::Arc::new(bendclaw::sessions::backend::noop::NoopBackend),
        run_initializer: std::sync::Arc::new(bendclaw::sessions::backend::noop::NoopBackend),
        skill_executor: std::sync::Arc::new(bendclaw::execution::skills::NoopSkillExecutor),
    });

    assert!(session.belongs_to("a1", "u1"));
    assert!(!session.belongs_to("a2", "u1"));
    assert!(!session.belongs_to("a1", "u2"));
    assert!(!session.belongs_to("a2", "u2"));
    Ok(())
}
