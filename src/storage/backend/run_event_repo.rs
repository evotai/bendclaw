use async_trait::async_trait;

use crate::base::entities::RunEvent;
use crate::base::Result;

#[async_trait]
pub trait RunEventRepo: Send + Sync {
    async fn append_event(&self, event: &RunEvent) -> Result<()>;
    async fn list_events_by_run(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
        run_id: &str,
    ) -> Result<Vec<RunEvent>>;
}
