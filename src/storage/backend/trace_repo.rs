use async_trait::async_trait;

use crate::base::entities::Trace;
use crate::base::Result;

#[async_trait]
pub trait TraceRepo: Send + Sync {
    async fn get_trace(
        &self,
        user_id: &str,
        agent_id: &str,
        trace_id: &str,
    ) -> Result<Option<Trace>>;
    async fn save_trace(&self, trace: &Trace) -> Result<()>;
    async fn list_traces_by_run(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
        run_id: &str,
    ) -> Result<Vec<Trace>>;
    async fn list_traces_by_session(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
    ) -> Result<Vec<Trace>>;
}
