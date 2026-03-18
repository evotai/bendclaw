use std::sync::Arc;

use tokio::sync::Mutex;

use crate::base::Result;
use crate::kernel::run::usage::CostSummary;
use crate::kernel::run::usage::UsageEvent;
use crate::kernel::run::usage::UsageScope;
use crate::llm::provider::LLMProvider;
use crate::storage::dal::usage::record::UsageRecord;
use crate::storage::dal::usage::repo::UsageRepo;
use crate::storage::time::now;

const FALLBACK_INPUT: f64 = 3.0;
const FALLBACK_OUTPUT: f64 = 15.0;
const FLUSH_THRESHOLD: usize = 50;

pub(super) struct UsageStore {
    usage_repo: UsageRepo,
    usage_buffer: Arc<Mutex<Vec<UsageRecord>>>,
    llm: Arc<dyn LLMProvider>,
}

impl UsageStore {
    pub fn new(usage_repo: UsageRepo, llm: Arc<dyn LLMProvider>) -> Self {
        Self {
            usage_repo,
            usage_buffer: Arc::new(Mutex::new(Vec::new())),
            llm,
        }
    }

    pub async fn record(&self, event: UsageEvent) -> Result<()> {
        let record = self.event_to_record(event);
        tracing::debug!(
            stage = "usage_store",
            action = "record",
            status = "buffered",
            user_id = %record.user_id, session_id = %record.session_id,
            run_id = %record.run_id, provider = %record.provider, model = %record.model, model_role = %record.model_role,
            prompt_tokens = record.prompt_tokens, completion_tokens = record.completion_tokens,
            reasoning_tokens = record.reasoning_tokens, ttft_ms = record.ttft_ms,
            cost = record.cost, "usage recorded"
        );
        let should_flush = {
            let mut buf = self.usage_buffer.lock().await;
            buf.push(record);
            buf.len() >= FLUSH_THRESHOLD
        };
        if should_flush {
            self.flush().await?;
        }
        Ok(())
    }

    pub async fn summarize(&self, scope: UsageScope) -> Result<CostSummary> {
        match scope {
            UsageScope::User { user_id } => self.usage_repo.summary_by_user(&user_id).await,
            UsageScope::AgentTotal { agent_id } => {
                self.usage_repo.summary_by_agent(&agent_id).await
            }
            UsageScope::AgentDaily { agent_id, day } => {
                self.usage_repo.summary_by_agent_day(&agent_id, &day).await
            }
        }
    }

    pub async fn flush(&self) -> Result<()> {
        const MAX_RETRIES: usize = 3;
        let mut records = {
            let mut buf = self.usage_buffer.lock().await;
            std::mem::take(&mut *buf)
        };
        if records.is_empty() {
            return Ok(());
        }
        tracing::debug!(count = records.len(), "flushing usage records");
        for attempt in 1..=MAX_RETRIES {
            match self.usage_repo.save_batch(&records).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!(stage = "usage_store", action = "flush", status = "retrying", error = %e, count = records.len(), attempt, "usage flush failed");
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            (attempt as u64) * 100,
                        ))
                        .await;
                    }
                }
            }
        }
        let mut buf = self.usage_buffer.lock().await;
        records.append(&mut *buf);
        *buf = records;
        tracing::warn!(
            stage = "usage_store",
            action = "flush",
            status = "requeued",
            count = buf.len(),
            "usage flush failed after retries; re-queued"
        );
        Ok(())
    }

    fn event_to_record(&self, event: UsageEvent) -> UsageRecord {
        let cost = if event.cost > 0.0 {
            event.cost
        } else {
            let (ip, op) = self
                .llm
                .pricing(&event.model)
                .unwrap_or((FALLBACK_INPUT, FALLBACK_OUTPUT));
            (event.prompt_tokens as f64 * ip + event.completion_tokens as f64 * op) / 1_000_000.0
        };
        UsageRecord {
            id: crate::kernel::new_id(),
            agent_id: event.agent_id,
            user_id: event.user_id,
            session_id: event.session_id,
            run_id: event.run_id,
            provider: event.provider,
            model: event.model,
            model_role: event.model_role.as_str().to_string(),
            prompt_tokens: event.prompt_tokens,
            completion_tokens: event.completion_tokens,
            reasoning_tokens: event.reasoning_tokens,
            total_tokens: event.prompt_tokens + event.completion_tokens,
            cache_read_tokens: event.cache_read_tokens,
            cache_write_tokens: event.cache_write_tokens,
            ttft_ms: event.ttft_ms,
            cost,
            created_at: now().to_rfc3339(),
        }
    }
}
