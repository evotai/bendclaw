pub mod ask_user;
pub mod bash;
pub mod edit;
pub mod file;
pub mod list;
pub mod memory;
pub mod search;
pub mod skill;
pub mod web_fetch;

pub use ask_user::AskUserFn;
pub use ask_user::AskUserOption;
pub use ask_user::AskUserRequest;
pub use ask_user::AskUserResponse;
pub use ask_user::AskUserTool;
pub use bash::BashTool;
pub use edit::EditFileTool;
pub use file::ReadFileTool;
pub use file::WriteFileTool;
pub use list::ListFilesTool;
pub use search::SearchTool;
pub use web_fetch::WebFetchTool;

use crate::types::AgentTool;

/// Base tools for a coding agent.
pub fn base_tools(envs: Vec<(String, String)>) -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(BashTool::default().with_envs(envs)),
        Box::new(ReadFileTool::default()),
        Box::new(WriteFileTool::new()),
        Box::new(EditFileTool::new()),
        Box::new(ListFilesTool::default()),
        Box::new(SearchTool::default()),
        Box::new(WebFetchTool::new()),
    ]
}

/// Tools for planning mode — exploration + optional user interaction.
///
/// Mutating tools (`write_file`, `edit_file`) remain visible to the LLM but
/// are disallowed: their `execute()` returns the given `disallow_message`
/// instead of performing the operation.
///
/// When `ask_fn` is `Some`, the `ask_user` tool is registered so the LLM can
/// present structured questions. When `None`, the tool is omitted and the LLM
/// proceeds without user interaction (graceful degradation).
pub fn planning_tools(
    ask_fn: Option<AskUserFn>,
    disallow_message: &str,
    envs: Vec<(String, String)>,
) -> Vec<Box<dyn AgentTool>> {
    let mut tools: Vec<Box<dyn AgentTool>> = vec![
        Box::new(BashTool::default().with_envs(envs)),
        Box::new(ReadFileTool::default()),
        Box::new(WriteFileTool::new().disallow(disallow_message)),
        Box::new(EditFileTool::new().disallow(disallow_message)),
        Box::new(ListFilesTool::default()),
        Box::new(SearchTool::default()),
        Box::new(WebFetchTool::new()),
    ];
    if let Some(f) = ask_fn {
        tools.push(Box::new(AskUserTool::new(f)));
    }
    tools
}

/// Read-only tools for side conversations (log analysis, inspection, etc.).
/// No mutation, no execution, no network.
pub fn readonly_tools() -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(ReadFileTool::default()),
        Box::new(ListFilesTool::default()),
        Box::new(SearchTool::default()),
    ]
}

/// Backward-compatible alias for the full normal tool set.
pub fn default_tools() -> Vec<Box<dyn AgentTool>> {
    base_tools(Vec::new())
}
