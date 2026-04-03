use std::collections::HashSet;
use std::sync::Arc;

use crate::kernel::channels::runtime::channel_registry::ChannelRegistry;
use crate::kernel::cluster::ClusterService;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::memory::MemoryService;
use crate::storage::Pool;
use crate::tools::definition::tool_definition::ToolDefinition;
use crate::tools::definition::tool_target::ToolTarget;
use crate::tools::definition::toolset::ToolEntry;
use crate::tools::definition::toolset::Toolset;
use crate::tools::selection::local_toolset::build_core_entries;
use crate::tools::tool_services::SecretUsageSink;

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

pub fn build_cloud_toolset(deps: CloudToolsetDeps, filter: Option<HashSet<String>>) -> Toolset {
    let mut entries = build_core_entries(deps.secret_sink.clone());
    entries.extend(build_cloud_entries(&deps));
    entries.extend(build_optional_entries(
        deps.cluster.as_ref(),
        deps.memory.as_ref(),
    ));

    let mut toolset = Toolset::from_entries(entries, filter);

    let skill_entries: Vec<ToolEntry> = deps
        .org
        .catalog()
        .visible_skills(&deps.user_id)
        .into_iter()
        .filter(|s| s.executable)
        .map(|s| {
            let name = crate::kernel::skills::definition::tool_key::format(&s, &deps.user_id);
            let desc = s.description.clone();
            let params = s.to_json_schema();
            ToolEntry {
                definition: ToolDefinition::from_skill(name, desc, params),
                target: ToolTarget::Skill,
            }
        })
        .collect();

    toolset.append_skill_entries(skill_entries);
    toolset
}

fn build_cloud_entries(deps: &CloudToolsetDeps) -> Vec<ToolEntry> {
    use crate::tools::channel::ChannelSendTool;
    use crate::tools::databend::DatabendTool;
    use crate::tools::skills::skill_create::SkillCreateTool;
    use crate::tools::skills::skill_read::SkillReadTool;
    use crate::tools::skills::skill_remove::SkillRemoveTool;
    use crate::tools::tasks::task_create::TaskCreateTool;
    use crate::tools::tasks::task_delete::TaskDeleteTool;
    use crate::tools::tasks::task_get::TaskGetTool;
    use crate::tools::tasks::task_history::TaskHistoryTool;
    use crate::tools::tasks::task_list::TaskListTool;
    use crate::tools::tasks::task_run::TaskRunTool;
    use crate::tools::tasks::task_toggle::TaskToggleTool;
    use crate::tools::tasks::task_update::TaskUpdateTool;

    let node_id = &deps.node_id;
    let pool = &deps.databend_pool;

    let tools: Vec<Arc<dyn crate::tools::Tool>> = vec![
        Arc::new(SkillReadTool::new(deps.org.catalog().clone())),
        Arc::new(SkillCreateTool::new(deps.org.manager().clone())),
        Arc::new(SkillRemoveTool::new(deps.org.manager().clone())),
        Arc::new(DatabendTool::new(pool.clone())),
        Arc::new(ChannelSendTool::new(deps.channels.clone(), pool.clone())),
        Arc::new(TaskCreateTool::new(node_id.clone(), pool.clone())),
        Arc::new(TaskListTool::new(node_id.clone(), pool.clone())),
        Arc::new(TaskGetTool::new(node_id.clone(), pool.clone())),
        Arc::new(TaskUpdateTool::new(node_id.clone(), pool.clone())),
        Arc::new(TaskDeleteTool::new(node_id.clone(), pool.clone())),
        Arc::new(TaskToggleTool::new(node_id.clone(), pool.clone())),
        Arc::new(TaskHistoryTool::new(node_id.clone(), pool.clone())),
        Arc::new(TaskRunTool::new(pool.clone())),
    ];

    tools
        .into_iter()
        .map(|t| ToolEntry {
            definition: ToolDefinition::from_builtin(t.as_ref()),
            target: ToolTarget::Builtin(t),
        })
        .collect()
}

fn build_optional_entries(
    cluster: Option<&(Arc<ClusterService>, Arc<DispatchTable>)>,
    memory: Option<&Arc<MemoryService>>,
) -> Vec<ToolEntry> {
    let mut entries = Vec::new();

    if let Some((service, dispatch_table)) = cluster {
        use crate::tools::cluster::cluster_collect::ClusterCollectTool;
        use crate::tools::cluster::cluster_dispatch::ClusterDispatchTool;
        use crate::tools::cluster::cluster_nodes::ClusterNodesTool;

        let tools: Vec<Arc<dyn crate::tools::Tool>> = vec![
            Arc::new(ClusterNodesTool::new(service.clone())),
            Arc::new(ClusterDispatchTool::new(
                service.clone(),
                dispatch_table.clone(),
            )),
            Arc::new(ClusterCollectTool::new(dispatch_table.clone())),
        ];
        entries.extend(tools.into_iter().map(|t| ToolEntry {
            definition: ToolDefinition::from_builtin(t.as_ref()),
            target: ToolTarget::Builtin(t),
        }));
    }

    if let Some(mem) = memory {
        use crate::tools::memory::memory_save::MemorySaveTool;
        use crate::tools::memory::memory_search::MemorySearchTool;

        let tools: Vec<Arc<dyn crate::tools::Tool>> = vec![
            Arc::new(MemorySearchTool::new(mem.clone())),
            Arc::new(MemorySaveTool::new(mem.clone())),
        ];
        entries.extend(tools.into_iter().map(|t| ToolEntry {
            definition: ToolDefinition::from_builtin(t.as_ref()),
            target: ToolTarget::Builtin(t),
        }));
    }

    entries
}
