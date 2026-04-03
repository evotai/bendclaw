//! CLI-specific adapter: converts CLI args into a standard request and
//! submits through the unified `binding::submit` pipeline.
//!
//! This module does NOT contain any execution logic — it is a thin
//! translation layer from `AgentRequest` to `binding::submit::submit_turn()`.

use std::sync::Arc;

use crate::binding::submit::submit_turn;
use crate::binding::submit::SubmitResult;
use crate::request::AgentRequest;
use crate::runtime::Runtime;
use crate::storage::sessions::Session;
use crate::types::id::new_id;
use crate::types::Result;

/// Submit a CLI request through the unified pipeline.
///
/// Converts the `AgentRequest` + bound `Session` into a `submit_turn()` call,
/// which is the same entry point used by HTTP and channel ingress.
pub async fn submit_cli_run(
    runtime: &Arc<Runtime>,
    request: &AgentRequest,
    session: &Session,
) -> Result<SubmitResult> {
    let trace_id = new_id();
    submit_turn(
        runtime,
        &session.agent_id,
        &session.session_id,
        &session.user_id,
        &request.prompt,
        &trace_id,
        None,
        "",
        "",
        false,
    )
    .await
}
