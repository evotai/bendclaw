use async_trait::async_trait;

use crate::error::Result;
use crate::session::SessionMeta;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn save_meta(&self, meta: &SessionMeta) -> Result<()>;
    async fn load_meta(&self, session_id: &str) -> Result<Option<SessionMeta>>;
    async fn list_recent(&self, limit: usize) -> Result<Vec<SessionMeta>>;
    async fn save_transcript(
        &self,
        session_id: &str,
        messages: &[bend_agent::Message],
    ) -> Result<()>;
    async fn load_transcript(&self, session_id: &str) -> Result<Option<Vec<bend_agent::Message>>>;
}
