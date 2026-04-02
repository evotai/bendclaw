use async_trait::async_trait;

use crate::types::entities::Session;
use crate::types::Result;

#[async_trait]
pub trait SessionRepo: Send + Sync {
    async fn find_session(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
    ) -> Result<Option<Session>>;
    async fn find_latest_session(&self, user_id: &str, agent_id: &str) -> Result<Option<Session>>;
    async fn create_session(&self, session: &Session) -> Result<()>;
    async fn update_session(&self, session: &Session) -> Result<()>;
    async fn list_sessions(&self, user_id: &str, agent_id: &str) -> Result<Vec<Session>>;
}
