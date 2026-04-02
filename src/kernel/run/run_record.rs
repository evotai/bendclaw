//! Run record initialization for a chat turn.

use std::sync::Arc;

use crate::base::Result;
use crate::kernel::run::persist::persist_op::PersistOp;
use crate::kernel::run::persist::persist_op::PersistWriter;
use crate::kernel::session::store::SessionStore;

/// Create session record (first turn) and run record. Returns the run_id.
/// DB writes are fire-and-forget via the background PersistWriter.
#[allow(clippy::too_many_arguments, dead_code)]
pub(crate) fn init_run(
    storage: &Arc<dyn SessionStore>,
    persist_writer: &PersistWriter,
    session_id: &str,
    agent_id: &str,
    user_id: &str,
    user_message: &str,
    parent_run_id: Option<&str>,
    node_id: &str,
) -> Result<String> {
    let run_id = crate::kernel::new_run_id();

    persist_writer.send(PersistOp::InitRun {
        storage: storage.clone(),
        run_id: run_id.clone(),
        session_id: session_id.to_string(),
        agent_id: agent_id.to_string(),
        user_id: user_id.to_string(),
        user_message: user_message.to_string(),
        parent_run_id: parent_run_id.unwrap_or_default().to_string(),
        node_id: node_id.to_string(),
    });

    Ok(run_id)
}
