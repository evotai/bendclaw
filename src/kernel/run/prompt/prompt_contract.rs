use async_trait::async_trait;

use crate::base::Result;
use crate::kernel::run::prompt::model::PromptRequestMeta;

#[async_trait]
pub trait PromptResolver: Send + Sync {
    async fn resolve(&self, meta: &PromptRequestMeta) -> Result<String>;
}
