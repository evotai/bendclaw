/// Pure identity data for tool execution labeling.
///
/// No methods that reach into observability — just fields.
/// Recorder and diagnostics build ServerCtx / audit payloads from these internally.
#[derive(Clone, Debug)]
pub struct ExecutionLabels {
    pub trace_id: String,
    pub run_id: String,
    pub session_id: String,
    pub agent_id: String,
}
