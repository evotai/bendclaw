use std::sync::Arc;

use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::kernel::skills::projector::SkillProjector;
use bendclaw::kernel::tools::execution::registry::tool_registry::ToolRegistry;
use bendclaw::kernel::tools::execution::tool_services::NoopSecretUsageSink;
use bendclaw::kernel::tools::ToolId;

fn make_registry() -> Arc<ToolRegistry> {
    let pool = bendclaw::storage::Pool::new("https://api.databend.com/v1", "test-token", "default")
        .expect("pool: static URL is always valid");
    let dir = std::env::temp_dir().join(format!("bendclaw-reg-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    let skill_store = Arc::new(
        bendclaw::kernel::skills::shared::DatabendSharedSkillStore::new(
            pool.with_database("evotai_meta")
                .expect("meta pool for projector"),
        ),
    );
    let projector = Arc::new(SkillProjector::new(
        dir,
        skill_store,
        Arc::new(bendclaw_test_harness::mocks::skill::NoopSubscriptionStore),
        None,
    ));
    let config = AgentConfig::default();
    let llm: Arc<dyn bendclaw::llm::provider::LLMProvider> =
        Arc::new(bendclaw_test_harness::mocks::llm::MockLLMProvider::with_text("ok"));
    let meta_pool = pool.with_database("evotai_meta").expect("meta pool");
    let org = Arc::new(OrgServices::new(meta_pool, projector, &config, llm));
    let channels = Arc::new(bendclaw::kernel::channel::registry::ChannelRegistry::new());
    let secret_sink: Arc<dyn bendclaw::kernel::tools::execution::tool_services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);

    let toolset = bendclaw::kernel::tools::execution::registry::toolset::build_cloud_toolset(
        bendclaw::kernel::tools::execution::registry::toolset::CloudToolsetDeps {
            org,
            databend_pool: pool,
            channels,
            node_id: "test_instance".to_string(),
            cluster: None,
            memory: None,
            secret_sink,
            user_id: "test-user".to_string(),
        },
        None,
    );
    toolset.registry
}

#[test]
fn session_tools_registers_all_builtins() {
    let registry = make_registry();
    let expected = ToolId::ALL;
    for id in expected {
        assert!(
            registry.get(id.as_str()).is_some(),
            "missing tool: {}",
            id.as_str()
        );
    }
}

#[test]
fn registry_list_returns_all_names() {
    let registry = make_registry();
    // core + cloud tools
    assert!(!registry.list().is_empty());
}

#[test]
fn registry_get_unknown_returns_none() {
    let registry = make_registry();
    assert!(registry.get("nonexistent_tool").is_none());
}

#[test]
fn registry_tool_schemas_count() {
    let registry = make_registry();
    let schemas = registry.tool_schemas();
    assert!(!schemas.is_empty());
}

#[test]
fn registry_get_by_ids() {
    let registry = make_registry();
    let schemas = registry.get_by_ids(&[ToolId::Bash, ToolId::Read]);
    assert_eq!(schemas.len(), 2);
    let names: Vec<&str> = schemas.iter().map(|s| s.function.name.as_str()).collect();
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"read"));
}

#[test]
fn registry_get_by_names() {
    let registry = make_registry();
    let schemas = registry.get_by_names(&["bash", "write", "nonexistent"]);
    assert_eq!(schemas.len(), 2);
}

#[test]
fn empty_registry() {
    let registry = ToolRegistry::new();
    assert!(registry.list().is_empty());
    assert!(registry.get("bash").is_none());
    assert!(registry.tool_schemas().is_empty());
}
