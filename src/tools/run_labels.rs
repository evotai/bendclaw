/// Per-run identity labels for the tool runtime layer (recorder, messages, diagnostics).
///
/// Carries trace_id + run_id so the runtime can stamp spans and audit events.
/// Distinct from `session::build::session_capabilities::RunLabels` which is session-scoped
/// (agent_id + user_id + session_id) and used during session assembly.
#[derive(Clone, Debug)]
pub struct RunLabels {
    pub trace_id: String,
    pub run_id: String,
    pub session_id: String,
    pub agent_id: String,
}
