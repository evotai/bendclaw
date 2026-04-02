use async_trait::async_trait;

use crate::types::entities::TaskHistory;
use crate::types::Result;

#[async_trait]
pub trait TaskHistoryRepo: Send + Sync {
    async fn append_history(&self, entry: &TaskHistory) -> Result<()>;
    async fn list_history_by_task(
        &self,
        user_id: &str,
        agent_id: &str,
        task_id: &str,
    ) -> Result<Vec<TaskHistory>>;
}
