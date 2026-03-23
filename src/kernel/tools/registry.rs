use std::collections::HashMap;
use std::sync::Arc;

use super::Tool;
use super::ToolId;
use super::ToolSpec;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::cluster::ClusterService;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::recall::RecallStore;
use crate::kernel::skills::remote::repository::DatabendSkillRepositoryFactory;
use crate::kernel::skills::store::SkillStore;
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
    skill_store: Arc<SkillStore>,
    skill_store_factory: Arc<DatabendSkillRepositoryFactory>,
    databend_pool: crate::storage::Pool,
    channels: Arc<ChannelRegistry>,
    node_id: String,
    recall_store: Arc<RecallStore>,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    // Memory tools
    let memory_backend: Arc<dyn super::memory::MemoryBackend> = storage.clone();
    registry.register_builtin(
        ToolId::MemoryWrite,
        Arc::new(super::memory::MemoryWriteTool::new(memory_backend.clone())),
    );
    registry.register_builtin(
        ToolId::MemorySearch,
        Arc::new(super::memory::MemorySearchTool::new(memory_backend.clone())),
    );
    registry.register_builtin(
        ToolId::MemoryRead,
        Arc::new(super::memory::MemoryReadTool::new(memory_backend.clone())),
    );
    registry.register_builtin(
        ToolId::MemoryDelete,
        Arc::new(super::memory::MemoryDeleteTool::new(memory_backend.clone())),
    );
    registry.register_builtin(
        ToolId::MemoryList,
        Arc::new(super::memory::MemoryListTool::new(memory_backend)),
    );

    // Skill tools
    registry.register_builtin(
        ToolId::SkillRead,
        Arc::new(super::skill::SkillReadTool::new(skill_store.clone())),
    );
    registry.register_builtin(
        ToolId::SkillCreate,
        Arc::new(super::skill::SkillCreateTool::new(
            skill_store_factory.clone(),
            skill_store.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::SkillRemove,
        Arc::new(super::skill::SkillRemoveTool::new(
            skill_store_factory,
            skill_store,
        )),
    );

    // File tools (zero-field — workspace comes from ToolContext at execution time)
    registry.register_builtin(ToolId::FileRead, Arc::new(super::file::FileReadTool));
    registry.register_builtin(ToolId::FileWrite, Arc::new(super::file::FileWriteTool));
    registry.register_builtin(ToolId::FileEdit, Arc::new(super::file::FileEditTool));
    registry.register_builtin(ToolId::ListDir, Arc::new(super::file::ListDirTool));

    // Search tools
    registry.register_builtin(ToolId::Grep, Arc::new(super::builtins::search::GrepTool));
    registry.register_builtin(ToolId::Glob, Arc::new(super::builtins::search::GlobTool));

    // Shell tool (zero-field — workspace comes from ToolContext at execution time)
    registry.register_builtin(ToolId::Shell, Arc::new(super::shell::ShellTool));

    // Web tools (zero-field — API key from VariableRepo at execution time)
    registry.register_builtin(
        ToolId::WebSearch,
        Arc::new(super::web::WebSearchTool::default()),
    );
    registry.register_builtin(ToolId::WebFetch, Arc::new(super::web::WebFetchTool));

    // Databend tool
    registry.register_builtin(
        ToolId::Databend,
        Arc::new(super::databend::DatabendTool::new(databend_pool)),
    );

    // Channel send tool
    registry.register_builtin(
        ToolId::ChannelSend,
        Arc::new(super::builtins::channel::ChannelSendTool::new(channels)),
    );

    // Task tools
    registry.register_builtin(
        ToolId::TaskCreate,
        Arc::new(super::task::TaskCreateTool::new(node_id.clone())),
    );
    registry.register_builtin(
        ToolId::TaskList,
        Arc::new(super::task::TaskListTool::new(node_id.clone())),
    );
    registry.register_builtin(
        ToolId::TaskGet,
        Arc::new(super::task::TaskGetTool::new(node_id.clone())),
    );
    registry.register_builtin(
        ToolId::TaskUpdate,
        Arc::new(super::task::TaskUpdateTool::new(node_id.clone())),
    );
    registry.register_builtin(
        ToolId::TaskDelete,
        Arc::new(super::task::TaskDeleteTool::new(node_id.clone())),
    );
    registry.register_builtin(
        ToolId::TaskToggle,
        Arc::new(super::task::TaskToggleTool::new(node_id.clone())),
    );
    registry.register_builtin(
        ToolId::TaskHistory,
        Arc::new(super::task::TaskHistoryTool::new(node_id)),
    );
    registry.register_builtin(ToolId::TaskRun, Arc::new(super::task::TaskRunTool));

    // Recall tools
    registry.register_builtin(
        ToolId::LearningWrite,
        Arc::new(super::recall::LearningWriteTool::new(recall_store.clone())),
    );
    registry.register_builtin(
        ToolId::KnowledgeSearch,
        Arc::new(super::recall::KnowledgeSearchTool::new(
            recall_store.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::LearningSearch,
        Arc::new(super::recall::LearningSearchTool::new(recall_store)),
    );

    // Coding agent tools
    registry.register_builtin(
        ToolId::ClaudeCode,
        Arc::new(super::coding_agent::ClaudeCodeTool),
    );
    registry.register_builtin(
        ToolId::CodexExec,
        Arc::new(super::coding_agent::CodexExecTool),
    );

    registry
}

/// Register cluster tools into an existing registry. Called conditionally
/// when cluster config is present.
pub fn register_cluster_tools(
    registry: &mut ToolRegistry,
    service: Arc<ClusterService>,
    dispatch_table: Arc<DispatchTable>,
) {
    registry.register_builtin(
        ToolId::ClusterNodes,
        Arc::new(super::builtins::cluster::ClusterNodesTool::new(
            service.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::ClusterDispatch,
        Arc::new(super::builtins::cluster::ClusterDispatchTool::new(
            service,
            dispatch_table.clone(),
        )),
    );
    registry.register_builtin(
        ToolId::ClusterCollect,
        Arc::new(super::builtins::cluster::ClusterCollectTool::new(
            dispatch_table,
        )),
    );
}
