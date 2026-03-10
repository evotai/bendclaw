use std::sync::Arc;

use bendclaw::kernel::tools::registry::create_session_tools;
use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw::kernel::tools::ToolId;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use bendclaw_test_harness::mocks::skill::NoopSkillCatalog;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;

fn make_registry() -> ToolRegistry {
    let factory = Arc::new(FixedStoreFactory);
    let pool = bendclaw::storage::Pool::new("https://api.databend.com/v1", "test-token", "default")
        .expect("pool: static URL is always valid");
    let llm: Arc<dyn bendclaw::llm::provider::LLMProvider> =
        Arc::new(MockLLMProvider::with_text("ok"));
    let storage = Arc::new(bendclaw::kernel::agent_store::AgentStore::new(
        pool.clone(),
        llm,
    ));
    let channels = Arc::new(bendclaw::kernel::channel::registry::ChannelRegistry::new());
    create_session_tools(
        storage,
        Arc::new(NoopSkillCatalog),
        factory,
        pool,
        channels,
        "test_instance".to_string(),
    )
}

struct FixedStoreFactory;

impl bendclaw::kernel::skills::repository::SkillRepositoryFactory for FixedStoreFactory {
    fn for_agent(
        &self,
        _agent_id: &str,
    ) -> bendclaw::base::Result<Arc<dyn bendclaw::kernel::skills::repository::SkillRepository>>
    {
        Ok(Arc::new(NoopSkillStore))
    }
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
    assert_eq!(registry.list().len(), ToolId::ALL.len());
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
    assert_eq!(schemas.len(), ToolId::ALL.len());
}

#[test]
fn registry_get_by_ids() {
    let registry = make_registry();
    let schemas = registry.get_by_ids(&[ToolId::Shell, ToolId::FileRead]);
    assert_eq!(schemas.len(), 2);
    let names: Vec<&str> = schemas.iter().map(|s| s.function.name.as_str()).collect();
    assert!(names.contains(&"shell"));
    assert!(names.contains(&"file_read"));
}

#[test]
fn registry_get_by_names() {
    let registry = make_registry();
    let schemas = registry.get_by_names(&["shell", "file_write", "nonexistent"]);
    assert_eq!(schemas.len(), 2);
}

#[test]
fn empty_registry() {
    let registry = ToolRegistry::new();
    assert!(registry.list().is_empty());
    assert!(registry.get("shell").is_none());
    assert!(registry.tool_schemas().is_empty());
}
