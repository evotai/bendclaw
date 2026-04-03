//! Persistent cloud session acquisition — cache, staleness, assembly.

use std::sync::Arc;

use crate::binding::session_builder::CloudBuildOptions;
use crate::binding::session_builder::SessionBuilder;
use crate::runtime::diagnostics;
use crate::runtime::Runtime;
use crate::sessions::build::session_capabilities::SessionOwner;
use crate::sessions::Session;
use crate::types::ErrorCode;
use crate::types::Result;

/// Acquire a persistent cloud session by identity. Used by server-side callers
/// (session_router, task executor) and the invocation layer's Persistent branch.
pub async fn acquire_cloud_session(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    session_id: &str,
    user_id: &str,
) -> Result<Arc<Session>> {
    acquire_cloud_session_with_opts(
        runtime,
        agent_id,
        session_id,
        user_id,
        CloudBuildOptions::default(),
    )
    .await
}

/// Acquire with explicit build options (cwd, tool_filter, llm_override).
pub async fn acquire_cloud_session_with_opts(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    session_id: &str,
    user_id: &str,
    opts: CloudBuildOptions,
) -> Result<Arc<Session>> {
    // Cache hit — check ownership and staleness
    if let Some(session) = runtime.sessions.get(session_id) {
        if !session.belongs_to(agent_id, user_id) {
            diagnostics::log_runtime_denied(agent_id, user_id, session_id);
            return Err(ErrorCode::denied(format!(
                "session '{session_id}' belongs to a different agent/user"
            )));
        }
        if session.is_stale() && !session.is_running() {
            runtime.sessions.remove(session_id);
            diagnostics::log_runtime_recreated(agent_id, user_id, session_id);
        } else {
            diagnostics::log_runtime_reused(agent_id, user_id, session_id);
            return Ok(session);
        }
    }

    // Cache miss or stale eviction — assemble, create, insert
    let owner = SessionOwner {
        agent_id: agent_id.to_string(),
        user_id: user_id.to_string(),
    };
    let assembly = SessionBuilder {
        runtime: runtime.clone(),
    }
    .build_cloud(session_id, &owner, opts)
    .await?;
    let tool_count = assembly.core.toolset.tools.len();
    let session = Arc::new(Session::from_assembly(assembly));
    runtime.sessions.insert(session.clone());

    diagnostics::log_runtime_session_created(
        agent_id,
        user_id,
        session_id,
        &runtime
            .config
            .workspace
            .session_dir(user_id, agent_id, session_id)
            .display()
            .to_string(),
        tool_count,
    );

    Ok(session)
}
