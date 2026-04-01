//! cmd_run — local oneshot/resume command.

use std::path::PathBuf;

use crate::base::Result;
use crate::kernel::session::assembly::local::build_local_assembly;
use crate::kernel::session::assembly::local::LocalBuildOptions;
use crate::kernel::session::assembly::local::LocalRuntimeDeps;
use crate::kernel::session::core::session::Session;
use crate::kernel::session::runtime::run_options::RunOptions;
use crate::local::args::RunArgs;

pub async fn execute(args: RunArgs, deps: LocalRuntimeDeps) -> Result<()> {
    let cwd = args.cwd.map(PathBuf::from);
    let system = args.system.as_deref().map(resolve_input).transpose()?;
    let tool_filter =
        crate::kernel::tools::execution::registry::tool_selection::parse_tool_selection(
            &args.tools,
        );

    let session_id = args.session_id.unwrap_or_else(crate::kernel::new_id);
    let prompt = resolve_input(&args.prompt)?;

    let assembly = build_local_assembly(&deps, &session_id, LocalBuildOptions {
        cwd,
        tool_filter,
        llm_override: None,
    })?;

    let session = Session::from_assembly(assembly);

    let options = RunOptions {
        system_overlay: system,
        max_iterations: Some(args.max_turns),
        max_duration_secs: Some(args.max_duration),
        ..Default::default()
    };

    let stream = session.run_with_options(&prompt, options).await?;
    let text = stream.finish().await?;
    println!("{text}");

    Ok(())
}

fn resolve_input(input: &str) -> Result<String> {
    if let Some(path) = input.strip_prefix('@') {
        std::fs::read_to_string(path)
            .map_err(|e| crate::base::ErrorCode::internal(format!("read {path}: {e}")))
    } else {
        Ok(input.to_string())
    }
}
