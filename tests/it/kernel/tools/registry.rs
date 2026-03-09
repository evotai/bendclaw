use std::sync::Arc;

use bendclaw::kernel::tools::registry::create_session_tools;
use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw::kernel::tools::ToolId;

use crate::mocks::llm::MockLLMProvider;
use crate::mocks::skill::NoopSkillCatalog;
use crate::mocks::skill::NoopSkillStore;

fn make_registry() -> ToolRegistry {
    let factory = Arc::new(FixedStoreFactory);
    let pool =
        bendclaw::storage::Pool::new("https://app.databend.com/v1.1", "test-token", "default")
            .unwrap_or_else(|_| panic!("pool"));
    let llm: Arc<dyn bendclaw::llm::provider::LLMProvider> =
        Arc::new(MockLLMProvider::with_text("ok"));
    let storage = Arc::new(bendclaw::kernel::agent_store::AgentStore::new(
        pool.clone(),
        llm,
    ));
    create_session_tools(storage, Arc::new(NoopSkillCatalog), factory, pool)
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
    let expected = [
        ToolId::MemoryWrite,
        ToolId::MemorySearch,
        ToolId::MemoryRead,
        ToolId::MemoryDelete,
        ToolId::MemoryList,
        ToolId::SkillRead,
        ToolId::SkillCreate,
        ToolId::SkillRemove,
        ToolId::FileRead,
        ToolId::FileWrite,
        ToolId::FileEdit,
        ToolId::Shell,
        ToolId::Databend,
    ];
    for id in &expected {
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
    assert_eq!(registry.list().len(), 13);
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
    assert_eq!(schemas.len(), 13);
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
