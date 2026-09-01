//! Tool-set construction: engine-owned built-ins plus host-delegated tools.

use std::path::PathBuf;

use evot_engine::host::HostTool;
use evot_engine::host::HostToolSpec;
use evot_engine::host::SharedHost;
use evot_engine::tools::*;

use super::ToolMode;

/// Host-owned tools attached to a run.
///
/// Bundles the outbound [`SharedHost`] bridge with the specs the host has
/// registered. The engine builds one [`HostTool`] per spec, all routing back
/// through the same bridge. This is the single seam for ask_user and any future
/// domain tool — none of them are known to the engine or app core.
#[derive(Clone)]
pub struct HostTools {
    pub host: SharedHost,
    pub specs: Vec<HostToolSpec>,
}

impl HostTools {
    pub fn new(host: SharedHost, specs: Vec<HostToolSpec>) -> Self {
        Self { host, specs }
    }

    fn into_tools(self) -> Vec<Box<dyn evot_engine::AgentTool>> {
        self.specs
            .into_iter()
            .map(|spec| {
                Box::new(HostTool::new(spec, self.host.clone())) as Box<dyn evot_engine::AgentTool>
            })
            .collect()
    }
}

fn build_bash_tool(
    envs: Vec<(String, String)>,
    sandbox_dirs: Option<Vec<PathBuf>>,
    process_manager: Option<std::sync::Arc<ProcessManager>>,
) -> Box<dyn evot_engine::AgentTool> {
    let mut bash = BashTool::default().with_envs(envs);
    if let Some(process_manager) = process_manager {
        bash = bash.with_process_manager(process_manager);
    }
    if let Some(dirs) = sandbox_dirs {
        bash = bash.with_sandbox_dirs(dirs);
    }
    Box::new(bash)
}

/// Assemble the active tool set for a turn.
///
/// `host_tools` carries the host bridge and its registered specs; it is
/// attached only when the mode allows it (see [`ToolMode::allows_host_tools`]).
pub(crate) fn build_tools(
    mode: ToolMode,
    envs: Vec<(String, String)>,
    allow_bash: bool,
    sandbox_dirs: Option<Vec<PathBuf>>,
    process_manager: Option<std::sync::Arc<ProcessManager>>,
    host_tools: Option<HostTools>,
) -> Vec<Box<dyn evot_engine::AgentTool>> {
    if matches!(mode, ToolMode::Readonly) {
        // Read-only mode: read + structured search, no mutation or shell.
        return vec![
            Box::new(ReadFileTool::default()),
            Box::new(GrepTool::new()),
            Box::new(GlobTool::new()),
            Box::new(SearchTool::new()),
        ];
    }

    let mut t: Vec<Box<dyn evot_engine::AgentTool>> = Vec::new();

    // Core coding set: read, bash, edit, write. The dedicated explore tools
    // (grep, glob, semantic_code_search) are currently unregistered here —
    // search and file-finding go through bash (rg/grep/find). They remain
    // available in Readonly mode above, which has no shell.
    t.push(Box::new(ReadFileTool::default()));

    if allow_bash {
        let background_enabled = mode.allows_background_processes() && process_manager.is_some();
        t.push(build_bash_tool(
            envs,
            sandbox_dirs,
            process_manager.clone().filter(|_| background_enabled),
        ));
        if let Some(process_manager) = process_manager.filter(|_| background_enabled) {
            t.push(Box::new(TaskOutputTool::new(process_manager.clone())));
            t.push(Box::new(TaskStopTool::new(process_manager)));
        }
    }

    if matches!(mode, ToolMode::Planning) {
        let msg = "Not allowed in planning mode. Use /act to switch.";
        t.push(Box::new(EditFileTool::new().disallow(msg)));
        t.push(Box::new(WriteFileTool::new().disallow(msg)));
    } else {
        t.push(Box::new(EditFileTool::new()));
        t.push(Box::new(WriteFileTool::new()));
    }

    // evot-specific tools, appended after the pi-aligned core set.
    if !matches!(mode, ToolMode::Headless) {
        t.push(Box::new(WebFetchTool::new()));
    }

    // Host-owned tools (ask_user, …). The engine treats these exactly like
    // built-ins; only their execution is delegated back to the host.
    if mode.allows_host_tools() {
        if let Some(host_tools) = host_tools {
            t.extend(host_tools.into_tools());
        }
    }

    t
}

/// Canonical tool set used to assemble the system prompt's tool list and
/// guidelines at startup for the gateway, which runs in Headless mode. Must
/// stay in sync with the Headless set built by `build_tools`: read, bash,
/// edit, write (no explore tools, no webfetch, no host tools).
pub(crate) fn prompt_tools() -> Vec<Box<dyn evot_engine::AgentTool>> {
    vec![
        Box::new(ReadFileTool::default()),
        Box::new(BashTool::default()),
        Box::new(EditFileTool::new()),
        Box::new(WriteFileTool::new()),
    ]
}
