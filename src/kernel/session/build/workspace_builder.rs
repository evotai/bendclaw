use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::session::workspace::OpenResolver;
use crate::kernel::session::workspace::SandboxResolver;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::variables::Variable;
use crate::llm::tool::ToolSchema;

/// Build workspace from a pre-computed workspace_dir + config policy.
/// Both local and cloud assemblers call this — policy (sandbox, resolver, timeouts)
/// lives here once, only the workspace_dir derivation differs per assembler.
pub fn build_workspace_from_dir(
    config: &AgentConfig,
    workspace_dir: PathBuf,
    cwd_override: Option<&Path>,
    variables: &[Variable],
) -> crate::base::Result<Arc<Workspace>> {
    if let Err(e) = std::fs::create_dir_all(&workspace_dir) {
        return Err(crate::base::ErrorCode::internal(format!(
            "failed to create session workspace: {e}"
        )));
    }

    let cwd = if config.workspace.sandbox {
        // Sandbox: always use workspace_dir, ignore external cwd override
        workspace_dir.clone()
    } else {
        cwd_override.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| workspace_dir.clone())
        })
    };

    let resolver: Arc<dyn crate::kernel::session::workspace::PathResolver> =
        if config.workspace.sandbox {
            Arc::new(SandboxResolver)
        } else {
            Arc::new(OpenResolver)
        };

    Ok(Arc::new(Workspace::from_variables(
        workspace_dir,
        cwd,
        config.workspace.safe_env_vars.clone(),
        variables,
        Duration::from_secs(config.workspace.command_timeout_secs),
        Duration::from_secs(config.workspace.max_command_timeout_secs),
        config.workspace.max_output_bytes,
        resolver,
    )))
}

/// Build workspace from config + variables. Cloud assembler uses this —
/// workspace_dir derived from config.workspace.session_dir().
pub fn build_workspace(
    config: &AgentConfig,
    agent_id: &str,
    session_id: &str,
    user_id: &str,
    cwd_override: Option<&Path>,
    variables: &[Variable],
) -> crate::base::Result<Arc<Workspace>> {
    let workspace_dir = config.workspace.session_dir(user_id, agent_id, session_id);
    build_workspace_from_dir(config, workspace_dir, cwd_override, variables)
}

/// Apply tool filter to schemas. Returns the allowed_tool_names set if a filter was given.
pub fn apply_tool_filter(
    tools: &mut Vec<ToolSchema>,
    filter: Option<HashSet<String>>,
) -> Option<HashSet<String>> {
    filter.map(|f| {
        tools.retain(|t| f.contains(&t.function.name));
        tools.iter().map(|t| t.function.name.clone()).collect()
    })
}
