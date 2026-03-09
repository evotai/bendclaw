use std::collections::HashMap;
use std::sync::Arc;

use super::Tool;
use super::ToolId;
use super::ToolSpec;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::skills::catalog::SkillCatalog;
use crate::kernel::skills::repository::SkillRepositoryFactory;
use crate::llm::tool::ToolSchema;

/// Registry of in-process tools, keyed by name.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn register_builtin(&mut self, id: ToolId, tool: Arc<dyn Tool>) {
        debug_assert_eq!(
            id.as_str(),
            tool.name(),
            "tool id/name mismatch for built-in tool"
        );
        self.register(tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tools.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        let mut specs: Vec<ToolSpec> = self.tools.values().map(|t| t.spec()).collect();
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    pub fn tool_schemas(&self) -> Vec<ToolSchema> {
        let mut schemas: Vec<ToolSchema> = self
            .tools
            .values()
            .map(|t| ToolSchema::new(t.name(), t.description(), t.parameters_schema()))
            .collect();
        schemas.sort_by(|a, b| a.function.name.cmp(&b.function.name));
        schemas
    }

    pub fn get_by_names(&self, names: &[&str]) -> Vec<ToolSchema> {
        names
            .iter()
            .filter_map(|name| {
                self.tools
                    .get(*name)
                    .map(|t| ToolSchema::new(t.name(), t.description(), t.parameters_schema()))
            })
            .collect()
    }

    pub fn get_by_ids(&self, ids: &[ToolId]) -> Vec<ToolSchema> {
        ids.iter()
            .filter_map(|id| {
                self.tools
                    .get(id.as_str())
                    .map(|t| ToolSchema::new(t.name(), t.description(), t.parameters_schema()))
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a per-session tool registry with all dependencies injected.
pub fn create_session_tools(
    storage: Arc<AgentStore>,
    skill_catalog: Arc<dyn SkillCatalog>,
    skill_store_factory: Arc<dyn SkillRepositoryFactory>,
    databend_pool: crate::storage::Pool,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    // Memory tools
    registry.register_builtin(
        ToolId::MemoryWrite,
        Arc::new(super::memory::MemoryWriteTool::new(storage.clone())),
    );
    registry.register_builtin(
        ToolId::MemorySearch,
        Arc::new(super::memory::MemorySearchTool::new(storage.clone())),
    );
    registry.register_builtin(
        ToolId::MemoryRead,
        Arc::new(super::memory::MemoryReadTool::new(storage.clone())),
    );
    registry.register_builtin(
        ToolId::MemoryDelete,
        Arc::new(super::memory::MemoryDeleteTool::new(storage.clone())),
    );
    registry.register_builtin(
        ToolId::MemoryList,
        Arc::new(super::memory::MemoryListTool::new(storage)),
    );

    // Skill tools
    registry.register_builtin(
        ToolId::SkillRead,
        Arc::new(super::skill::SkillReadTool::new(skill_catalog.clone())),
    );
    registry.register_builtin(
        ToolId::SkillCreate,
        Arc::new(super::skill::SkillCreateTool::new(
            skill_store_factory.clone(),
            skill_catalog.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::SkillRemove,
        Arc::new(super::skill::SkillRemoveTool::new(
            skill_store_factory,
            skill_catalog,
        )),
    );

    // File tools (zero-field — workspace comes from ToolContext at execution time)
    registry.register_builtin(ToolId::FileRead, Arc::new(super::file::FileReadTool));
    registry.register_builtin(ToolId::FileWrite, Arc::new(super::file::FileWriteTool));
    registry.register_builtin(ToolId::FileEdit, Arc::new(super::file::FileEditTool));

    // Shell tool (zero-field — workspace comes from ToolContext at execution time)
    registry.register_builtin(ToolId::Shell, Arc::new(super::shell::ShellTool));

    // Databend tool
    registry.register_builtin(
        ToolId::Databend,
        Arc::new(super::databend::DatabendTool::new(databend_pool)),
    );

    registry
}
