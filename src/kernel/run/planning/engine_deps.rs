use std::sync::Arc;

use crate::kernel::memory::MemoryService;
use crate::kernel::run::execution::skills::SkillExecutor;
use crate::kernel::run::hooks::BeforeTurnHook;
use crate::kernel::run::hooks::SteeringSource;
use crate::kernel::session::runtime::session_resources::SessionResources;
use crate::kernel::tools::definition::toolset::Toolset;

/// Narrow projection of SessionResources — only what run assembly needs.
pub struct RunAssemblyDeps {
    pub workspace: Arc<crate::kernel::session::workspace::Workspace>,
    pub toolset: Toolset,
    pub skill_executor: Arc<dyn SkillExecutor>,
    pub tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    pub extract_memory: Option<Arc<MemoryService>>,
    pub before_turn_hook: Option<Arc<dyn BeforeTurnHook>>,
    pub steering_source: Option<Arc<dyn SteeringSource>>,
}

impl RunAssemblyDeps {
    pub fn from_resources(r: &SessionResources) -> Self {
        Self {
            workspace: r.workspace.clone(),
            toolset: r.toolset.clone(),
            skill_executor: r.skill_executor.clone(),
            tool_writer: r.tool_writer.clone(),
            extract_memory: r.org.memory().filter(|_| r.config.memory.extract),
            before_turn_hook: r.before_turn_hook.clone(),
            steering_source: r.steering_source.clone(),
        }
    }
}
