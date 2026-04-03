//! SessionStore — minimal persistence contract for session/run data.
//!
//! Both local (JSON) and cloud (Databend) implement this trait.
//! Session core, PersistOp, TurnPersister, and PersistentBackend
//! depend only on this trait — never on AgentStore directly.

use async_trait::async_trait;

use crate::execution::usage::CostSummary;
use crate::execution::usage::UsageEvent;
use crate::execution::usage::UsageScope;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run::record::RunStatus;
use crate::storage::dal::run_event::record::RunEventRecord;
use crate::storage::dal::session::record::SessionRecord;
use crate::storage::dal::session::repo::SessionWrite;
use crate::types::Result;

#[async_trait]
#[allow(clippy::too_many_arguments)]
pub trait SessionStore: Send + Sync {
    // ── Session ────────────────────────────────────────────────────
    async fn session_load(&self, id: &str) -> Result<Option<SessionRecord>>;
    async fn session_upsert(&self, record: SessionWrite) -> Result<()>;

    // ── Run ────────────────────────────────────────────────────────
    async fn run_insert(&self, record: &RunRecord) -> Result<()>;
    async fn run_update_final(
        &self,
        run_id: &str,
        status: RunStatus,
        output: &str,
        error: &str,
        metrics: &str,
        stop_reason: &str,
        iterations: u32,
    ) -> Result<()>;
    async fn run_update_status(&self, run_id: &str, status: RunStatus) -> Result<()>;
    async fn run_list_by_session(&self, session_id: &str, limit: u32) -> Result<Vec<RunRecord>>;
    async fn run_load_latest_checkpoint(&self, session_id: &str) -> Result<Option<RunRecord>>;

    // ── Events ─────────────────────────────────────────────────────
    async fn run_events_insert_batch(&self, records: &[RunEventRecord]) -> Result<()>;

    // ── Usage ──────────────────────────────────────────────────────
    async fn usage_record(&self, event: UsageEvent) -> Result<()>;
    async fn usage_flush(&self) -> Result<()>;
    async fn usage_summarize(&self, scope: UsageScope) -> Result<CostSummary>;
}
