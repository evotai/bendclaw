use std::collections::HashSet;
use std::sync::Arc;

use super::tool_registry::ToolRegistry;
use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::cluster::ClusterService;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::memory::MemoryService;
use crate::kernel::tools::execution::tool_services::SecretUsageSink;
use crate::kernel::tools::ToolId;
use crate::llm::tool::ToolSchema;
use crate::storage::Pool;

const CORE_TOOLS: &[ToolId] = &[
    ToolId::Read,
    ToolId::Write,
    ToolId::Edit,
    ToolId::Bash,
    ToolId::Glob,
    ToolId::Grep,
    ToolId::WebFetch,
    ToolId::WebSearch,
];

#[derive(Clone)]
pub struct Toolset {
    pub registry: Arc<ToolRegistry>,
    pub tools: Arc<Vec<ToolSchema>>,
    pub allowed_tool_names: Option<HashSet<String>>,
}

pub struct CloudToolsetDeps {
    pub org: Arc<crate::kernel::runtime::org::OrgServices>,
    pub databend_pool: Pool,
    pub channels: Arc<ChannelRegistry>,
    pub node_id: String,
    pub cluster: Option<(Arc<ClusterService>, Arc<DispatchTable>)>,
    pub memory: Option<Arc<MemoryService>>,
    pub secret_sink: Arc<dyn SecretUsageSink>,
    pub user_id: String,
}

pub fn build_local_toolset(
    filter: Option<HashSet<String>>,
    secret_sink: Arc<dyn SecretUsageSink>,
) -> Toolset {
    let mut registry = ToolRegistry::new();
    register_core(&mut registry, secret_sink);
    finish_toolset(registry, filter)
}

pub fn build_cloud_toolset(deps: CloudToolsetDeps, filter: Option<HashSet<String>>) -> Toolset {
    let mut registry = ToolRegistry::new();
    register_core(&mut registry, deps.secret_sink.clone());
    register_cloud(&mut registry, &deps);
    register_optional(&mut registry, deps.cluster.as_ref(), deps.memory.as_ref());

    let mut toolset = finish_toolset(registry, filter.clone());

    let skills: Vec<_> = deps
        .org
        .skills()
        .list(&deps.user_id)
        .into_iter()
        .filter(|s| s.executable)
        .map(|s| {
            let name = crate::kernel::skills::tool_key::format(&s, &deps.user_id);
            let desc = s.description.clone();
            let params = s.to_json_schema();
            (name, desc, params)
        })
        .collect();

    append_skill_schemas(&mut toolset, &skills, &filter);
    toolset
}

pub fn append_skill_schemas(
    toolset: &mut Toolset,
    skills: &[(String, String, serde_json::Value)],
    filter: &Option<HashSet<String>>,
) {
    let existing_names: HashSet<String> = toolset
        .tools
        .iter()
        .map(|t| t.function.name.clone())
        .collect();
    let mut tools = toolset.tools.as_ref().clone();
    for (name, desc, params) in skills {
        if existing_names.contains(name) {
            continue;
        }
        if let Some(ref f) = filter {
            if !f.contains(name) {
                continue;
            }
        }
        tools.push(ToolSchema::new(name, desc, params.clone()));
        if let Some(ref mut allowed) = toolset.allowed_tool_names {
            allowed.insert(name.clone());
        }
    }
    toolset.tools = Arc::new(tools);
}

fn finish_toolset(registry: ToolRegistry, filter: Option<HashSet<String>>) -> Toolset {
    let registry = Arc::new(registry);
    let (tools, allowed_tool_names) = match filter {
        Some(names) => {
            let schemas: Vec<ToolSchema> = registry
                .tool_schemas()
                .into_iter()
                .filter(|t| names.contains(&t.function.name))
                .collect();
            let allowed: HashSet<String> =
                schemas.iter().map(|t| t.function.name.clone()).collect();
            (schemas, Some(allowed))
        }
        None => {
            let schemas = registry.get_by_ids(CORE_TOOLS);
            (schemas, None)
        }
    };
    Toolset {
        registry,
        tools: Arc::new(tools),
        allowed_tool_names,
    }
}

fn register_core(registry: &mut ToolRegistry, secret_sink: Arc<dyn SecretUsageSink>) {
    use crate::kernel::tools::bash::ShellTool;
    use crate::kernel::tools::edit::FileEditTool;
    use crate::kernel::tools::glob::GlobTool;
    use crate::kernel::tools::grep::GrepTool;
    use crate::kernel::tools::list_dir::ListDirTool;
    use crate::kernel::tools::read::FileReadTool;
    use crate::kernel::tools::web_fetch::WebFetchTool;
    use crate::kernel::tools::web_search::WebSearchTool;
    use crate::kernel::tools::write::FileWriteTool;

    registry.register_builtin(ToolId::Read, Arc::new(FileReadTool));
    registry.register_builtin(ToolId::Write, Arc::new(FileWriteTool));
    registry.register_builtin(ToolId::Edit, Arc::new(FileEditTool));
    registry.register_builtin(ToolId::ListDir, Arc::new(ListDirTool));
    registry.register_builtin(ToolId::Grep, Arc::new(GrepTool));
    registry.register_builtin(ToolId::Glob, Arc::new(GlobTool));
    registry.register_builtin(ToolId::Bash, Arc::new(ShellTool::new(secret_sink.clone())));
    registry.register_builtin(
        ToolId::WebSearch,
        Arc::new(WebSearchTool::new(
            "https://api.search.brave.com/res/v1/web/search",
            secret_sink,
        )),
    );
    registry.register_builtin(ToolId::WebFetch, Arc::new(WebFetchTool));
}

fn register_cloud(registry: &mut ToolRegistry, deps: &CloudToolsetDeps) {
    use crate::kernel::tools::channel_send::ChannelSendTool;
    use crate::kernel::tools::databend::DatabendTool;
    use crate::kernel::tools::skills::create::SkillCreateTool;
    use crate::kernel::tools::skills::read::SkillReadTool;
    use crate::kernel::tools::skills::remove::SkillRemoveTool;
    use crate::kernel::tools::tasks::create::TaskCreateTool;
    use crate::kernel::tools::tasks::delete::TaskDeleteTool;
    use crate::kernel::tools::tasks::get::TaskGetTool;
    use crate::kernel::tools::tasks::history::TaskHistoryTool;
    use crate::kernel::tools::tasks::list::TaskListTool;
    use crate::kernel::tools::tasks::run::TaskRunTool;
    use crate::kernel::tools::tasks::toggle::TaskToggleTool;
    use crate::kernel::tools::tasks::update::TaskUpdateTool;

    registry.register_builtin(
        ToolId::SkillRead,
        Arc::new(SkillReadTool::new(deps.org.skills().clone())),
    );
    registry.register_builtin(
        ToolId::SkillCreate,
        Arc::new(SkillCreateTool::new(deps.org.skills().clone())),
    );
    registry.register_builtin(
        ToolId::SkillRemove,
        Arc::new(SkillRemoveTool::new(deps.org.skills().clone())),
    );
    registry.register_builtin(
        ToolId::Databend,
        Arc::new(DatabendTool::new(deps.databend_pool.clone())),
    );
    registry.register_builtin(
        ToolId::ChannelSend,
        Arc::new(ChannelSendTool::new(
            deps.channels.clone(),
            deps.databend_pool.clone(),
        )),
    );
    let node_id = &deps.node_id;
    let pool = &deps.databend_pool;
    registry.register_builtin(
        ToolId::TaskCreate,
        Arc::new(TaskCreateTool::new(node_id.clone(), pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskList,
        Arc::new(TaskListTool::new(node_id.clone(), pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskGet,
        Arc::new(TaskGetTool::new(node_id.clone(), pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskUpdate,
        Arc::new(TaskUpdateTool::new(node_id.clone(), pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskDelete,
        Arc::new(TaskDeleteTool::new(node_id.clone(), pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskToggle,
        Arc::new(TaskToggleTool::new(node_id.clone(), pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskHistory,
        Arc::new(TaskHistoryTool::new(node_id.clone(), pool.clone())),
    );
    registry.register_builtin(ToolId::TaskRun, Arc::new(TaskRunTool::new(pool.clone())));
}

fn register_optional(
    registry: &mut ToolRegistry,
    cluster: Option<&(Arc<ClusterService>, Arc<DispatchTable>)>,
    memory: Option<&Arc<MemoryService>>,
) {
    if let Some((service, dispatch_table)) = cluster {
        use crate::kernel::tools::cluster::collect::ClusterCollectTool;
        use crate::kernel::tools::cluster::dispatch::ClusterDispatchTool;
        use crate::kernel::tools::cluster::nodes::ClusterNodesTool;

        registry.register_builtin(
            ToolId::ClusterNodes,
            Arc::new(ClusterNodesTool::new(service.clone())),
        );
        registry.register_builtin(
            ToolId::ClusterDispatch,
            Arc::new(ClusterDispatchTool::new(
                service.clone(),
                dispatch_table.clone(),
            )),
        );
        registry.register_builtin(
            ToolId::ClusterCollect,
            Arc::new(ClusterCollectTool::new(dispatch_table.clone())),
        );
    }

    if let Some(mem) = memory {
        use crate::kernel::tools::memory::save::MemorySaveTool;
        use crate::kernel::tools::memory::search::MemorySearchTool;

        registry.register_builtin(
            ToolId::MemorySearch,
            Arc::new(MemorySearchTool::new(mem.clone())),
        );
        registry.register_builtin(
            ToolId::MemorySave,
            Arc::new(MemorySaveTool::new(mem.clone())),
        );
    }
}
