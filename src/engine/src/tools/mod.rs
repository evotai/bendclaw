pub mod bash;
pub mod edit;
pub mod file;
pub mod list;
pub mod search;
pub mod web_fetch;

pub use bash::BashTool;
pub use edit::EditFileTool;
pub use file::ReadFileTool;
pub use file::WriteFileTool;
pub use list::ListFilesTool;
pub use search::SearchTool;
pub use web_fetch::WebFetchTool;

use crate::types::AgentTool;

/// Base tools for a coding agent.
pub fn base_tools() -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(BashTool::default()),
        Box::new(ReadFileTool::default()),
        Box::new(WriteFileTool::new()),
        Box::new(EditFileTool::new()),
        Box::new(ListFilesTool::default()),
        Box::new(SearchTool::default()),
        Box::new(WebFetchTool::new()),
    ]
}

/// Backward-compatible alias for the full normal tool set.
pub fn default_tools() -> Vec<Box<dyn AgentTool>> {
    base_tools()
}
