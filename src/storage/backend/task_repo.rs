use async_trait::async_trait;

use crate::base::entities::Task;
use crate::base::Result;

#[async_trait]
pub trait TaskRepo: Send + Sync {
    async fn get_task(&self, user_id: &str, agent_id: &str, task_id: &str) -> Result<Option<Task>>;
    async fn save_task(&self, task: &Task) -> Result<()>;
    async fn delete_task(&self, user_id: &str, agent_id: &str, task_id: &str) -> Result<()>;
    async fn list_tasks(&self, user_id: &str, agent_id: &str) -> Result<Vec<Task>>;
    async fn update_task(&self, task: &Task) -> Result<()>;
}
