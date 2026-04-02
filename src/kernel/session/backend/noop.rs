use async_trait::async_trait;

use super::context::SessionContextProvider;
use super::sink::RunInitializer;
use super::sink::RunPersister;
use crate::kernel::run::event::Event;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::Message;
use crate::types::ErrorCode;
use crate::types::Result;

/// No-op backend for ephemeral sessions. All operations are silent no-ops.
pub struct NoopBackend;

#[async_trait]
impl SessionContextProvider for NoopBackend {
    async fn load_history(&self, _limit: usize) -> Result<Vec<Message>> {
        Ok(vec![])
    }

    async fn enforce_token_limits(&self) -> Result<()> {
        Ok(())
    }
}

impl RunInitializer for NoopBackend {
    fn init_run(
        &self,
        _input: &str,
        _parent_run_id: Option<&str>,
        _node_id: &str,
    ) -> Result<String> {
        Ok(crate::types::id::new_run_id())
    }
}

impl RunPersister for NoopBackend {
    fn persist_success(
        &self,
        _result: AgentResult,
        _provider: &str,
        _model: &str,
        _events: &[Event],
    ) {
    }
    fn persist_error(&self, _error: &ErrorCode, _events: &[Event]) {}
    fn persist_cancelled(&self, _events: &[Event]) {}
}
