use async_trait::async_trait;

use crate::types::entities::Agent;
use crate::types::Result;

#[async_trait]
pub trait AgentRepo: Send + Sync {
    async fn get_agent(&self, user_id: &str, agent_id: &str) -> Result<Option<Agent>>;
    async fn save_agent(&self, agent: &Agent) -> Result<()>;
    async fn delete_agent(&self, user_id: &str, agent_id: &str) -> Result<()>;
    async fn list_agents(&self, user_id: &str) -> Result<Vec<Agent>>;
}
