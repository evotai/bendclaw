use std::sync::Arc;

use super::usage_store::UsageStore;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::agent_store::memory_store::DatabendMemoryStore;
use crate::kernel::agent_store::memory_store::MemoryEntry;
use crate::kernel::agent_store::memory_store::MemoryResult;
use crate::kernel::agent_store::memory_store::SearchOpts;
use crate::kernel::run::usage::UsageEvent;
use crate::kernel::run::usage::UsageScope;
use crate::llm::config::LLMConfig;
use crate::llm::provider::LLMProvider;
use crate::storage::dal::agent_config::record::AgentConfigRecord;
use crate::storage::dal::agent_config::repo::AgentConfigStore;
use crate::storage::dal::config_version::record::ConfigVersionRecord;
use crate::storage::dal::config_version::repo::ConfigVersionRepo;
use crate::storage::dal::learning::record::LearningRecord;
use crate::storage::dal::learning::repo::LearningRepo;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run::record::RunStatus;
use crate::storage::dal::run::repo::RunRepo;
use crate::storage::dal::run_event::record::RunEventRecord;
use crate::storage::dal::run_event::repo::RunEventRepo;
use crate::storage::dal::session::record::SessionRecord;
use crate::storage::dal::session::repo::SessionRepo;
use crate::storage::dal::trace::record::SpanRecord;
use crate::storage::dal::trace::repo::SpanRepo;
use crate::storage::dal::trace::repo::TraceRepo;
use crate::storage::dal::usage::repo::UsageRepo;
use crate::storage::dal::usage::types::CostSummary;
use crate::storage::dal::variable::VariableRecord;
use crate::storage::dal::variable::VariableRepo;
use crate::storage::pool::Pool;

pub struct AgentStore {
    pool: Pool,
    memory: DatabendMemoryStore,
    sessions: SessionRepo,
    runs: RunRepo,
    run_events: RunEventRepo,
    config: AgentConfigStore,
    traces: TraceRepo,
    spans: SpanRepo,
    usage: UsageStore,
}

impl AgentStore {
    pub fn new(pool: Pool, llm: Arc<dyn LLMProvider>) -> Self {
        Self {
            memory: DatabendMemoryStore::new(pool.clone()),
            sessions: SessionRepo::new(pool.clone()),
            runs: RunRepo::new(pool.clone()),
            run_events: RunEventRepo::new(pool.clone()),
            config: AgentConfigStore::new(pool.clone()),
            traces: TraceRepo::new(pool.clone()),
            spans: SpanRepo::new(pool.clone()),
            usage: UsageStore::new(UsageRepo::new(pool.clone()), llm),
            pool,
        }
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub fn trace_repo(&self) -> Arc<TraceRepo> {
        Arc::new(self.traces.clone())
    }

    pub fn span_repo(&self) -> Arc<SpanRepo> {
        Arc::new(self.spans.clone())
    }

    // ── Memory ────────────────────────────────────────────────────────────

    pub async fn memory_write(&self, user_id: &str, entry: MemoryEntry) -> Result<()> {
        self.memory.write(user_id, entry).await
    }

    pub async fn memory_search(
        &self,
        query: &str,
        user_id: &str,
        opts: SearchOpts,
    ) -> Result<Vec<MemoryResult>> {
        self.memory.search(query, user_id, opts).await
    }

    pub async fn memory_get(&self, user_id: &str, key: &str) -> Result<Option<MemoryEntry>> {
        self.memory.get(user_id, key).await
    }

    pub async fn memory_get_by_id(&self, user_id: &str, id: &str) -> Result<Option<MemoryEntry>> {
        self.memory.get_by_id(user_id, id).await
    }

    pub async fn memory_delete(&self, user_id: &str, id: &str) -> Result<()> {
        self.memory.delete(user_id, id).await
    }

    pub async fn memory_list(&self, user_id: &str, limit: u32) -> Result<Vec<MemoryEntry>> {
        self.memory.list(user_id, limit).await
    }

    // ── Sessions ──────────────────────────────────────────────────────────

    pub async fn session_upsert(
        &self,
        session_id: &str,
        agent_id: &str,
        user_id: &str,
        title: Option<&str>,
        meta: Option<&serde_json::Value>,
    ) -> Result<()> {
        self.sessions
            .upsert(session_id, agent_id, user_id, title, None, meta)
            .await
    }

    pub async fn session_load(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        self.sessions.load(session_id).await
    }

    pub async fn session_list_by_user(
        &self,
        user_id: &str,
        limit: u32,
    ) -> Result<Vec<SessionRecord>> {
        self.sessions.list_by_user(user_id, limit).await
    }

    pub async fn session_update_state(
        &self,
        session_id: &str,
        state: &serde_json::Value,
    ) -> Result<()> {
        self.sessions.update_state(session_id, state).await
    }

    pub async fn session_get_state(&self, session_id: &str) -> Result<serde_json::Value> {
        let record = self.sessions.load(session_id).await?;
        Ok(record
            .map(|r| r.session_state)
            .unwrap_or(serde_json::Value::Null))
    }

    pub async fn session_delete(&self, session_id: &str) -> Result<()> {
        self.sessions.delete_by_id(session_id).await
    }

    // ── Runs ──────────────────────────────────────────────────────────────

    pub async fn run_insert(&self, record: &RunRecord) -> Result<()> {
        self.runs.insert(record).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_update_final(
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

    pub async fn run_update_status(&self, run_id: &str, status: RunStatus) -> Result<()> {
        self.runs.update_status(run_id, status).await
    }

    pub async fn run_load(&self, run_id: &str) -> Result<Option<RunRecord>> {
        self.runs.load(run_id).await
    }

    pub async fn run_list_by_session(
        &self,
        session_id: &str,
        limit: u32,
    ) -> Result<Vec<RunRecord>> {
        self.runs.list_by_session(session_id, limit).await
    }

    pub async fn run_events_insert_batch(&self, records: &[RunEventRecord]) -> Result<()> {
        self.run_events.insert_batch(records).await
    }

    pub async fn run_events_list_by_run(
        &self,
        run_id: &str,
        limit: u32,
    ) -> Result<Vec<RunEventRecord>> {
        self.run_events.list_by_run(run_id, limit).await
    }

    // ── Traces ─────────────────────────────────────────────────────────────

    pub async fn recent_failed_spans(
        &self,
        session_id: &str,
        limit: u32,
    ) -> Result<Vec<SpanRecord>> {
        let sid = crate::storage::sql::escape(session_id);
        let cond = format!(
            "status = 'failed' AND trace_id IN \
             (SELECT trace_id FROM traces WHERE session_id = '{sid}')"
        );
        let repo = SpanRepo::new(self.pool.clone());
        repo.list_where(&cond, "created_at DESC", limit as u64)
            .await
    }

    // ── Learnings ──────────────────────────────────────────────────────────

    pub async fn learning_list(&self, limit: u32) -> Result<Vec<LearningRecord>> {
        let repo = LearningRepo::new(self.pool.clone());
        repo.list(limit).await
    }

    pub async fn learning_insert(&self, record: &LearningRecord) -> Result<()> {
        let repo = LearningRepo::new(self.pool.clone());
        repo.insert(record).await
    }

    pub async fn learning_delete(&self, learning_id: &str) -> Result<()> {
        let repo = LearningRepo::new(self.pool.clone());
        repo.delete(learning_id).await
    }

    // ── Config ────────────────────────────────────────────────────────────

    pub async fn config_get(&self, agent_id: &str) -> Result<Option<AgentConfigRecord>> {
        self.config.get(agent_id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn config_upsert(
        &self,
        agent_id: &str,
        system_prompt: Option<&str>,
        display_name: Option<&str>,
        description: Option<&str>,
        identity: Option<&str>,
        soul: Option<&str>,
        token_limit_total: Option<Option<u64>>,
        token_limit_daily: Option<Option<u64>>,
        llm_config: Option<Option<&LLMConfig>>,
    ) -> Result<()> {
        let llm_json = llm_config.map(|opt| {
            opt.map(|cfg| serde_json::to_string(cfg).unwrap_or_else(|_| "null".to_string()))
        });
        let llm_str: Option<&str> = match &llm_json {
            Some(Some(s)) => Some(s.as_str()),
            Some(None) => None,
            None => None,
        };
        self.config
            .upsert(
                agent_id,
                system_prompt,
                display_name,
                description,
                identity,
                soul,
                token_limit_total,
                token_limit_daily,
                llm_str,
            )
            .await
    }

    pub async fn config_get_system_prompt(&self, agent_id: &str) -> Result<String> {
        self.config.get_system_prompt(agent_id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn config_update_with_version(
        &self,
        agent_id: &str,
        system_prompt: Option<&str>,
        display_name: Option<&str>,
        description: Option<&str>,
        identity: Option<&str>,
        soul: Option<&str>,
        token_limit_total: Option<Option<u64>>,
        token_limit_daily: Option<Option<u64>>,
        llm_config: Option<Option<&LLMConfig>>,
        notes: Option<&str>,
        label: Option<&str>,
    ) -> Result<u32> {
        let llm_json = llm_config.map(|opt| {
            opt.map(|cfg| serde_json::to_string(cfg).unwrap_or_else(|_| "null".to_string()))
        });
        let llm_str: Option<&str> = match &llm_json {
            Some(Some(s)) => Some(s.as_str()),
            Some(None) => None,
            None => None,
        };
        self.config
            .upsert(
                agent_id,
                system_prompt,
                display_name,
                description,
                identity,
                soul,
                token_limit_total,
                token_limit_daily,
                llm_str,
            )
            .await?;

        let snapshot = self
            .config
            .get(agent_id)
            .await?
            .ok_or_else(|| ErrorCode::internal("agent config missing after upsert"))?;

        let version_repo = ConfigVersionRepo::new(self.pool.clone());
        let next = version_repo.next_version(agent_id).await?;
        let record = ConfigVersionRecord {
            id: crate::base::new_id(),
            agent_id: agent_id.to_string(),
            version: next,
            label: label.unwrap_or_default().to_string(),
            stage: "published".to_string(),
            system_prompt: snapshot.system_prompt,
            display_name: snapshot.display_name,
            description: snapshot.description,
            identity: snapshot.identity,
            soul: snapshot.soul,
            token_limit_total: snapshot.token_limit_total,
            token_limit_daily: snapshot.token_limit_daily,
            llm_config: snapshot.llm_config,
            notes: notes.unwrap_or_default().to_string(),
            created_at: String::new(),
        };
        version_repo.insert(&record).await.map(|_| next)
    }

    // ── Variables ──────────────────────────────────────────────────────────

    pub async fn variable_list(&self) -> Result<Vec<VariableRecord>> {
        let repo = VariableRepo::new(self.pool.clone());
        repo.list_all_active().await
    }

    // ── Usage ─────────────────────────────────────────────────────────────

    pub async fn usage_record(&self, event: UsageEvent) -> Result<()> {
        self.usage.record(event).await
    }

    pub async fn usage_summarize(&self, scope: UsageScope) -> Result<CostSummary> {
        self.usage.summarize(scope).await
    }

    pub async fn usage_flush(&self) -> Result<()> {
        self.usage.flush().await
    }
}
