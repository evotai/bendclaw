use std::collections::HashSet;
use std::sync::Arc;

use crate::tools::definition::tool_definition::ToolDefinition;
use crate::tools::definition::tool_target::ToolTarget;
use crate::tools::definition::toolset::ToolEntry;
use crate::tools::definition::toolset::Toolset;
use crate::tools::filesystem::FileEditTool;
use crate::tools::filesystem::FileReadTool;
use crate::tools::filesystem::FileWriteTool;
use crate::tools::filesystem::GlobTool;
use crate::tools::filesystem::GrepTool;
use crate::tools::filesystem::ListDirTool;
use crate::tools::shell::ShellTool;
use crate::tools::tool_services::SecretUsageSink;
use crate::tools::web::WebFetchTool;
use crate::tools::web::WebSearchTool;

pub fn build_local_toolset(
    filter: Option<HashSet<String>>,
    secret_sink: Arc<dyn SecretUsageSink>,
) -> Toolset {
    let entries = build_core_entries(secret_sink);
    Toolset::from_entries(entries, filter)
}

pub(crate) fn build_core_entries(secret_sink: Arc<dyn SecretUsageSink>) -> Vec<ToolEntry> {
    let tools: Vec<Arc<dyn crate::tools::Tool>> = vec![
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
