use std::collections::HashSet;
use std::sync::Arc;

use super::super::filesystem::FileEditTool;
use super::super::filesystem::FileReadTool;
use super::super::filesystem::FileWriteTool;
use super::super::filesystem::GlobTool;
use super::super::filesystem::GrepTool;
use super::super::filesystem::ListDirTool;
use super::super::shell::ShellTool;
use super::super::web::WebFetchTool;
use super::super::web::WebSearchTool;
use crate::kernel::tools::execution::registry::tool_registry::ToolRegistry;
use crate::kernel::tools::execution::registry::toolset::Toolset;
use crate::kernel::tools::execution::tool_services::SecretUsageSink;
use crate::kernel::tools::ToolId;

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

pub fn build_local_toolset(
    filter: Option<HashSet<String>>,
    secret_sink: Arc<dyn SecretUsageSink>,
) -> Toolset {
    let mut registry = ToolRegistry::new();
    register_core(&mut registry, secret_sink);
    Toolset::from_registry(registry, filter, CORE_TOOLS)
}

pub(crate) fn register_core(registry: &mut ToolRegistry, secret_sink: Arc<dyn SecretUsageSink>) {
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

pub(crate) const LOCAL_CORE_TOOLS: &[ToolId] = CORE_TOOLS;
