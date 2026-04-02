use async_trait::async_trait;

use crate::kernel::Message;
use crate::types::Result;

/// Provides session context data (history, quota enforcement).
/// PersistentBackend binds labels at construction; trait methods only take runtime params.
#[async_trait]
pub trait SessionContextProvider: Send + Sync {
    async fn load_history(&self, limit: usize) -> Result<Vec<Message>>;
    async fn enforce_token_limits(&self) -> Result<()>;
}
