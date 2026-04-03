//! Invocation-level session acquisition — cloud only.

use std::sync::Arc;

use crate::kernel::runtime::Runtime;
use crate::kernel::session::build::session_builder::CloudBuildOptions;
use crate::kernel::session::build::session_builder::SessionBuilder;
use crate::kernel::session::build::session_capabilities::SessionOwner;
use crate::kernel::session::Session;
use crate::request::invocation::*;
use crate::types::Result;

/// Acquire a one-shot session for the given invocation request.
pub async fn acquire_session(
    runtime: &Arc<Runtime>,
    req: &InvocationRequest,
) -> Result<Arc<Session>> {
    let session_id = crate::types::id::new_session_id();
    let owner = SessionOwner {
        agent_id: req.agent_id.clone(),
        user_id: req.user_id.clone(),
    };

    let assembly = SessionBuilder {
        runtime: runtime.clone(),
    }
    .build_cloud(&session_id, &owner, CloudBuildOptions {
        cwd: req.session_options.cwd.clone(),
        tool_filter: req.session_options.tool_filter.clone(),
        llm_override: req.session_options.llm_override.clone(),
    })
    .await?;

    Ok(Arc::new(Session::from_assembly(assembly)))
}
