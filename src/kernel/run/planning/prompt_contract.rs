use async_trait::async_trait;

use super::prompt_model::PromptRequestMeta;
use crate::base::Result;

#[async_trait]
pub trait PromptResolver: Send + Sync {
    async fn resolve(&self, meta: &PromptRequestMeta) -> Result<String>;
}
