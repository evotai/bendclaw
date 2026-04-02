use async_trait::async_trait;

use crate::base::entities::Run;
use crate::base::Result;

#[async_trait]
pub trait RunRepo: Send + Sync {
    async fn get_run(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
        run_id: &str,
    ) -> Result<Option<Run>>;
    async fn save_run(&self, run: &Run) -> Result<()>;
    async fn list_runs_by_session(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
    ) -> Result<Vec<Run>>;

    async fn load_handoff(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
        run_id: &str,
    ) -> Result<Option<serde_json::Value>>;
    async fn save_handoff(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
        run_id: &str,
        handoff: &serde_json::Value,
    ) -> Result<()>;
    async fn clear_handoff(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
        run_id: &str,
    ) -> Result<()>;
    async fn list_incomplete_runs(&self, user_id: &str, agent_id: &str) -> Result<Vec<Run>>;
}
