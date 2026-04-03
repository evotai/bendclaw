use async_trait::async_trait;

use crate::types::entities::Span;
use crate::types::Result;

#[async_trait]
pub trait SpanRepo: Send + Sync {
    async fn append_span(&self, span: &Span) -> Result<()>;
    async fn list_spans_by_trace(
        &self,
        user_id: &str,
        agent_id: &str,
        trace_id: &str,
    ) -> Result<Vec<Span>>;
}
