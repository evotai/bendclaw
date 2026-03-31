use std::sync::Arc;

use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::cluster::ClusterService;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::memory::MemoryService;
use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::services::SecretUsageSink;
use crate::kernel::tools::ToolId;
use crate::storage::Pool;

/// Register core tools: file, search, shell, web.
/// Zero platform dependencies — only need a SecretUsageSink for shell/web secret touch.
pub fn register_core(registry: &mut ToolRegistry, secret_sink: Arc<dyn SecretUsageSink>) {
    use crate::kernel::tools::file_edit::FileEditTool;
    use crate::kernel::tools::file_read::FileReadTool;
    use crate::kernel::tools::file_write::FileWriteTool;
    use crate::kernel::tools::glob::GlobTool;
    use crate::kernel::tools::grep::GrepTool;
    use crate::kernel::tools::list_dir::ListDirTool;
    use crate::kernel::tools::shell::ShellTool;
    use crate::kernel::tools::web_fetch::WebFetchTool;
    use crate::kernel::tools::web_search::WebSearchTool;

    registry.register_builtin(ToolId::FileRead, Arc::new(FileReadTool));
    registry.register_builtin(ToolId::FileWrite, Arc::new(FileWriteTool));
    registry.register_builtin(ToolId::FileEdit, Arc::new(FileEditTool));
    registry.register_builtin(ToolId::ListDir, Arc::new(ListDirTool));
    registry.register_builtin(ToolId::Grep, Arc::new(GrepTool));
    registry.register_builtin(ToolId::Glob, Arc::new(GlobTool));
    registry.register_builtin(ToolId::Shell, Arc::new(ShellTool::new(secret_sink.clone())));
    registry.register_builtin(
        ToolId::WebSearch,
        Arc::new(WebSearchTool::new(
            "https://api.search.brave.com/res/v1/web/search",
            secret_sink,
        )),
    );
    registry.register_builtin(ToolId::WebFetch, Arc::new(WebFetchTool));
}

/// Register tools that require Pool/OrgServices. Only for persistent sessions.
pub fn register_cloud(
    registry: &mut ToolRegistry,
    org: Arc<crate::kernel::runtime::org::OrgServices>,
    databend_pool: Pool,
    channels: Arc<ChannelRegistry>,
    node_id: String,
) {
    register_skill_tools(registry, &org);
    register_databend_and_channel(registry, &databend_pool, channels);
    register_task_tools(registry, node_id, databend_pool);
}

fn register_skill_tools(
    registry: &mut ToolRegistry,
    org: &Arc<crate::kernel::runtime::org::OrgServices>,
) {
    use crate::kernel::tools::skill_create::SkillCreateTool;
    use crate::kernel::tools::skill_read::SkillReadTool;
    use crate::kernel::tools::skill_remove::SkillRemoveTool;

    registry.register_builtin(
        ToolId::SkillRead,
        Arc::new(SkillReadTool::new(org.skills().clone())),
    );
    registry.register_builtin(
        ToolId::SkillCreate,
        Arc::new(SkillCreateTool::new(org.skills().clone())),
    );
    registry.register_builtin(
        ToolId::SkillRemove,
        Arc::new(SkillRemoveTool::new(org.skills().clone())),
    );
}

fn register_databend_and_channel(
    registry: &mut ToolRegistry,
    databend_pool: &Pool,
    channels: Arc<ChannelRegistry>,
) {
    use crate::kernel::tools::channel_send::ChannelSendTool;
    use crate::kernel::tools::databend::DatabendTool;

    registry.register_builtin(
        ToolId::Databend,
        Arc::new(DatabendTool::new(databend_pool.clone())),
    );
    registry.register_builtin(
        ToolId::ChannelSend,
        Arc::new(ChannelSendTool::new(channels, databend_pool.clone())),
    );
}

fn register_task_tools(registry: &mut ToolRegistry, node_id: String, databend_pool: Pool) {
    use crate::kernel::tools::task_create::TaskCreateTool;
    use crate::kernel::tools::task_delete::TaskDeleteTool;
    use crate::kernel::tools::task_get::TaskGetTool;
    use crate::kernel::tools::task_history::TaskHistoryTool;
    use crate::kernel::tools::task_list::TaskListTool;
    use crate::kernel::tools::task_run::TaskRunTool;
    use crate::kernel::tools::task_toggle::TaskToggleTool;
    use crate::kernel::tools::task_update::TaskUpdateTool;

    registry.register_builtin(
        ToolId::TaskCreate,
        Arc::new(TaskCreateTool::new(node_id.clone(), databend_pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskList,
        Arc::new(TaskListTool::new(node_id.clone(), databend_pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskGet,
        Arc::new(TaskGetTool::new(node_id.clone(), databend_pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskUpdate,
        Arc::new(TaskUpdateTool::new(node_id.clone(), databend_pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskDelete,
        Arc::new(TaskDeleteTool::new(node_id.clone(), databend_pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskToggle,
        Arc::new(TaskToggleTool::new(node_id.clone(), databend_pool.clone())),
    );
    registry.register_builtin(
        ToolId::TaskHistory,
        Arc::new(TaskHistoryTool::new(node_id, databend_pool.clone())),
    );
    registry.register_builtin(ToolId::TaskRun, Arc::new(TaskRunTool::new(databend_pool)));
}

/// Register optional tools that need extra services (cluster, memory).
/// Called conditionally based on service availability.
pub fn register_optional(
    registry: &mut ToolRegistry,
    cluster: Option<(&Arc<ClusterService>, &Arc<DispatchTable>)>,
    memory: Option<&Arc<MemoryService>>,
) {
    if let Some((service, dispatch_table)) = cluster {
        use crate::kernel::tools::cluster_collect::ClusterCollectTool;
        use crate::kernel::tools::cluster_dispatch::ClusterDispatchTool;
        use crate::kernel::tools::cluster_nodes::ClusterNodesTool;

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
        use crate::kernel::tools::memory_save::MemorySaveTool;
        use crate::kernel::tools::memory_search::MemorySearchTool;

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
