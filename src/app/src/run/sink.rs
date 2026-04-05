use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
use crate::storage::model::RunEvent;

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()>;
}
