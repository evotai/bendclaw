//! DbSessionStore — cloud/server SessionStore backed by Databend repos.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::contract::SessionStore;
use crate::execution::usage::CostSummary;
use crate::execution::usage::UsageEvent;
use crate::execution::usage::UsageScope;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run::record::RunStatus;
use crate::storage::dal::run::repo::RunRepo;
use crate::storage::dal::run_event::record::RunEventRecord;
use crate::storage::dal::run_event::repo::RunEventRepo;
use crate::storage::dal::session::record::SessionRecord;
use crate::storage::dal::session::repo::SessionRepo;
use crate::storage::dal::session::repo::SessionWrite;
use crate::storage::dal::usage::record::UsageRecord;
use crate::storage::dal::usage::repo::UsageRepo;
use crate::storage::time::now;
use crate::storage::Pool;
use crate::types::Result;

const FLUSH_THRESHOLD: usize = 50;

pub struct DbSessionStore {
    sessions: SessionRepo,
    runs: RunRepo,
    run_events: RunEventRepo,
    usage: UsageRepo,
    usage_buffer: Arc<Mutex<Vec<UsageRecord>>>,
}

impl DbSessionStore {
    pub fn new(pool: Pool) -> Self {
        Self {
            sessions: SessionRepo::new(pool.clone()),
            runs: RunRepo::new(pool.clone()),
            run_events: RunEventRepo::new(pool.clone()),
            usage: UsageRepo::new(pool),
            usage_buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn event_to_record(event: UsageEvent) -> UsageRecord {
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
            cost: event.cost,
            created_at: now().to_rfc3339(),
        }
    }
}

#[async_trait]
impl SessionStore for DbSessionStore {
    async fn session_load(&self, id: &str) -> Result<Option<SessionRecord>> {
        self.sessions.load(id).await
    }

    async fn session_upsert(&self, record: SessionWrite) -> Result<()> {
        self.sessions.upsert(record).await
    }

    async fn run_insert(&self, record: &RunRecord) -> Result<()> {
        self.runs.insert(record).await
    }

    async fn run_update_final(
        &self,
        run_id: &str,
        status: RunStatus,
        output: &str,
        error: &str,
        metrics: &str,
        stop_reason: &str,
        iterations: u32,
    ) -> Result<()> {
        self.runs
            .update_final(
                run_id,
                status,
                output,
                error,
                metrics,
                stop_reason,
                iterations,
            )
            .await
    }

    async fn run_update_status(&self, run_id: &str, status: RunStatus) -> Result<()> {
        self.runs.update_status(run_id, status).await
    }

    async fn run_list_by_session(&self, session_id: &str, limit: u32) -> Result<Vec<RunRecord>> {
        self.runs.list_by_session(session_id, limit).await
    }

    async fn run_load_latest_checkpoint(&self, session_id: &str) -> Result<Option<RunRecord>> {
        self.runs.load_latest_checkpoint(session_id).await
    }

    async fn run_events_insert_batch(&self, records: &[RunEventRecord]) -> Result<()> {
        self.run_events.insert_batch(records).await
    }

    async fn usage_record(&self, event: UsageEvent) -> Result<()> {
        let record = Self::event_to_record(event);
        let should_flush = {
            let mut buf = self.usage_buffer.lock().await;
            buf.push(record);
            buf.len() >= FLUSH_THRESHOLD
        };
        if should_flush {
            self.usage_flush().await?;
        }
        Ok(())
    }

    async fn usage_flush(&self) -> Result<()> {
        let records = {
            let mut buf = self.usage_buffer.lock().await;
            std::mem::take(&mut *buf)
        };
        if records.is_empty() {
            return Ok(());
        }
        self.usage.save_batch(&records).await
    }

    async fn usage_summarize(&self, scope: UsageScope) -> Result<CostSummary> {
        match scope {
            UsageScope::User { user_id } => self.usage.summary_by_user(&user_id).await,
            UsageScope::AgentTotal { agent_id } => self.usage.summary_by_agent(&agent_id).await,
            UsageScope::AgentDaily { agent_id, day } => {
                self.usage.summary_by_agent_day(&agent_id, &day).await
            }
        }
    }
}
