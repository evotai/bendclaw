use async_trait::async_trait;

use crate::error::Result;
use crate::run::RunEvent;
use crate::run::RunMeta;

#[async_trait]
pub trait RunStore: Send + Sync {
    async fn save_run(&self, meta: &RunMeta) -> Result<()>;
    async fn append_event(&self, run_id: &str, event: &RunEvent) -> Result<()>;
    async fn load_events(&self, run_id: &str) -> Result<Vec<RunEvent>>;
}
