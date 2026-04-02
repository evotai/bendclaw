use std::collections::HashSet;
use std::sync::Arc;

use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::cluster::ClusterService;
use crate::kernel::cluster::DispatchTable;
use crate::kernel::memory::MemoryService;
use crate::kernel::tools::catalog::local_catalog::register_core;
use crate::kernel::tools::catalog::local_catalog::LOCAL_CORE_TOOLS;
use crate::kernel::tools::catalog::optional_catalog::register_optional;
use crate::kernel::tools::catalog::skill_schemas::append_skill_schemas;
use crate::kernel::tools::catalog::tool_registry::ToolRegistry;
use crate::kernel::tools::catalog::toolset::Toolset;
use crate::kernel::tools::tool_services::SecretUsageSink;
use crate::kernel::tools::ToolId;
use crate::storage::Pool;

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
    let mut registry = ToolRegistry::new();
    register_core(&mut registry, deps.secret_sink.clone());
    register_cloud(&mut registry, &deps);
    register_optional(&mut registry, deps.cluster.as_ref(), deps.memory.as_ref());

    let mut toolset = Toolset::from_registry(registry, filter.clone(), LOCAL_CORE_TOOLS);

    let skills: Vec<_> = deps
        .org
        .catalog()
        .visible_skills(&deps.user_id)
        .into_iter()
        .filter(|s| s.executable)
        .map(|s| {
            let name = crate::kernel::skills::model::tool_key::format(&s, &deps.user_id);
            let desc = s.description.clone();
            let params = s.to_json_schema();
            (name, desc, params)
        })
        .collect();

    append_skill_schemas(&mut toolset, &skills, &filter);
    toolset
}

fn register_cloud(registry: &mut ToolRegistry, deps: &CloudToolsetDeps) {
    use crate::kernel::tools::builtin::channel::ChannelSendTool;
    use crate::kernel::tools::builtin::databend::DatabendTool;
    use crate::kernel::tools::builtin::skills::create::SkillCreateTool;
    use crate::kernel::tools::builtin::skills::read::SkillReadTool;
    use crate::kernel::tools::builtin::skills::remove::SkillRemoveTool;
    use crate::kernel::tools::builtin::tasks::create::TaskCreateTool;
    use crate::kernel::tools::builtin::tasks::delete::TaskDeleteTool;
    use crate::kernel::tools::builtin::tasks::get::TaskGetTool;
    use crate::kernel::tools::builtin::tasks::history::TaskHistoryTool;
    use crate::kernel::tools::builtin::tasks::list::TaskListTool;
    use crate::kernel::tools::builtin::tasks::run::TaskRunTool;
    use crate::kernel::tools::builtin::tasks::toggle::TaskToggleTool;
    use crate::kernel::tools::builtin::tasks::update::TaskUpdateTool;

    registry.register_builtin(
        ToolId::SkillRead,
        Arc::new(SkillReadTool::new(deps.org.catalog().clone())),
    );
    registry.register_builtin(
        ToolId::SkillCreate,
        Arc::new(SkillCreateTool::new(deps.org.manager().clone())),
    );
    registry.register_builtin(
        ToolId::SkillRemove,
        Arc::new(SkillRemoveTool::new(deps.org.manager().clone())),
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
