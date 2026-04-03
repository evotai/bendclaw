use crate::execution::event::Event;
use crate::execution::result::Result as AgentResult;
use crate::types::ErrorCode;

/// Session-level: creates a run record and returns the run_id.
pub trait RunInitializer: Send + Sync {
    fn init_run(
        &self,
        input: &str,
        parent_run_id: Option<&str>,
        node_id: &str,
    ) -> crate::types::Result<String>;
}

/// Per-run: persists the outcome of a single run (success/error/cancelled).
pub trait RunPersister: Send + Sync {
    fn persist_success(&self, result: AgentResult, provider: &str, model: &str, events: &[Event]);

    fn persist_error(&self, error: &ErrorCode, events: &[Event]);

    fn persist_cancelled(&self, events: &[Event]);
}
