use std::collections::HashSet;
use std::sync::Arc;

use crate::kernel::tools::builtin::filesystem::FileEditTool;
use crate::kernel::tools::builtin::filesystem::FileReadTool;
use crate::kernel::tools::builtin::filesystem::FileWriteTool;
use crate::kernel::tools::builtin::filesystem::GlobTool;
use crate::kernel::tools::builtin::filesystem::GrepTool;
use crate::kernel::tools::builtin::filesystem::ListDirTool;
use crate::kernel::tools::builtin::shell::ShellTool;
use crate::kernel::tools::builtin::web::WebFetchTool;
use crate::kernel::tools::builtin::web::WebSearchTool;
use crate::kernel::tools::definition::tool_definition::ToolDefinition;
use crate::kernel::tools::definition::tool_target::ToolTarget;
use crate::kernel::tools::definition::toolset::ToolEntry;
use crate::kernel::tools::definition::toolset::Toolset;
use crate::kernel::tools::tool_services::SecretUsageSink;

pub fn build_local_toolset(
    filter: Option<HashSet<String>>,
    secret_sink: Arc<dyn SecretUsageSink>,
) -> Toolset {
    let entries = build_core_entries(secret_sink);
    Toolset::from_entries(entries, filter)
}

pub(crate) fn build_core_entries(secret_sink: Arc<dyn SecretUsageSink>) -> Vec<ToolEntry> {
    let tools: Vec<Arc<dyn crate::kernel::tools::Tool>> = vec![
        Arc::new(FileReadTool),
        Arc::new(FileWriteTool),
        Arc::new(FileEditTool),
        Arc::new(ListDirTool),
        Arc::new(GrepTool),
        Arc::new(GlobTool),
        Arc::new(ShellTool::new(secret_sink.clone())),
        Arc::new(WebSearchTool::new(
            "https://api.search.brave.com/res/v1/web/search",
            secret_sink,
        )),
        Arc::new(WebFetchTool),
    ];

    tools
        .into_iter()
        .map(|t| ToolEntry {
            definition: ToolDefinition::from_builtin(t.as_ref()),
            target: ToolTarget::Builtin(t),
        })
        .collect()
}
