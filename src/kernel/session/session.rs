//! Session — the aggregate root for agent conversations.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::directive::DirectiveService;
use crate::kernel::recall::RecallStore;
use crate::kernel::run::compactor::Compactor;
use crate::kernel::run::context::Context;
use crate::kernel::run::dispatcher::ToolDispatcher;
use crate::kernel::run::engine::Engine;
use crate::kernel::run::event::Event;
use crate::kernel::run::persister::TurnPersister;
use crate::kernel::run::prompt::PromptBuilder;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::run::usage::UsageScope;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::session::session_manager::SessionInfo;
use crate::kernel::session::session_manager::TurnStats;
use crate::kernel::session::session_stream::Stream;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::skills::store::SkillStore;
use crate::kernel::tools::progressive::ProgressiveToolView;
use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::ToolContext;
use crate::kernel::trace::TraceRecorder;
use crate::kernel::Message;
use crate::llm::provider::LLMProvider;
use crate::llm::tool::ToolSchema;
use crate::observability::audit;
use crate::observability::server_log;

const USAGE_PROVIDER_UNKNOWN: &str = "unknown";

pub(crate) enum SessionState {
    Idle,
    Running {
        run_id: String,
        cancel: CancellationToken,
        started_at: Instant,
        iteration: Arc<AtomicU32>,
    },
}

pub struct SessionResources {
    pub workspace: Arc<Workspace>,
    pub tool_registry: Arc<ToolRegistry>,
    pub skills: Arc<SkillStore>,
    pub tools: Arc<Vec<ToolSchema>>,
    pub storage: Arc<AgentStore>,
    pub llm: Arc<RwLock<Arc<dyn LLMProvider>>>,
    pub config: Arc<AgentConfig>,
    pub variables: Vec<crate::storage::dal::variable::record::VariableRecord>,
    pub recall: Option<Arc<RecallStore>>,
    pub cluster_client: Option<Arc<crate::kernel::cluster::ClusterService>>,
    pub directive: Option<Arc<DirectiveService>>,
}

pub struct Session {
    pub id: String,
    agent_id: Arc<str>,
    user_id: Arc<str>,
    res: SessionResources,
    pub(crate) state: Arc<Mutex<SessionState>>,
    history: Arc<Mutex<Vec<Message>>>,
    last_active: Mutex<Instant>,
    stale: AtomicBool,
}

impl Session {
    pub fn new(id: String, agent_id: Arc<str>, user_id: Arc<str>, res: SessionResources) -> Self {
        Self {
            id,
            agent_id,
            user_id,
            res,
            state: Arc::new(Mutex::new(SessionState::Idle)),
            history: Arc::new(Mutex::new(Vec::new())),
            last_active: Mutex::new(Instant::now()),
            stale: AtomicBool::new(false),
        }
    }

    pub async fn run(
        &self,
        user_message: &str,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
    ) -> Result<Stream> {
        {
            let state = self.state.lock();
            if let SessionState::Running { run_id, .. } = &*state {
                tracing::warn!(
                    session_id = %self.id,
                    agent_id = %self.agent_id,
                    active_run_id = %run_id,
                    "rejected new run: session already has a running run"
                );
                return Err(ErrorCode::denied(format!(
                    "session already has a running run: {run_id}"
                )));
            }
        }
        *self.last_active.lock() = Instant::now();
        let start = Instant::now();

        self.enforce_token_limits().await?;

        let directive_prompt = self
            .res
            .directive
            .as_ref()
            .and_then(|directive| directive.cached_prompt());

        self.ensure_history_loaded().await?;
        let mut history = self.history.lock().clone();
        let run_index = history
            .iter()
            .filter(|m| matches!(m, Message::User { .. }))
            .count() as u32
            + 1;

        let run_id = crate::kernel::run::run_record::init_run(
            &self.res.storage,
            &self.id,
            &self.agent_id,
            &self.user_id,
            user_message,
            parent_run_id,
            &self.res.config.node_id,
        )
        .await?;

        history.push(Message::user(user_message));

        let trace = self
            .create_trace(&run_id, trace_id, parent_trace_id, origin_node_id)
            .await;
        let run_ctx = server_log::ServerCtx::new(
            &trace.trace_id,
            &run_id,
            &self.id,
            &self.agent_id,
            run_index,
        );
        server_log::info(
            &run_ctx,
            "run",
            "started",
            server_log::ServerFields::default()
                .bytes(user_message.len() as u64)
                .detail("user_id", self.user_id.to_string())
                .detail("run_index", run_index)
                .detail("parent_run_id", parent_run_id)
                .detail("input", user_message),
        );

        let full_prompt = {
            let mut pb = PromptBuilder::new(self.res.storage.clone(), self.res.skills.clone())
                .with_tools(self.res.tools.clone())
                .with_variables(self.res.variables.clone());
            if let Some(ref recall) = self.res.recall {
                pb = pb.with_recall(recall.clone());
            }
            if let Some(ref cc) = self.res.cluster_client {
                pb = pb.with_cluster_client(cc.clone());
            }
            pb = pb.with_directive_prompt(directive_prompt);
            pb.build(&self.agent_id, &self.user_id, &self.id).await?
        };

        server_log::info(
            &run_ctx,
            "prompt",
            "built",
            server_log::ServerFields::default()
                .bytes(full_prompt.len() as u64)
                .detail("user_id", self.user_id.to_string())
                .detail("tool_count", self.res.tools.len())
                .detail("history_messages", history.len())
                .detail("prompt", full_prompt.clone()),
        );

        let initial_events = vec![
            {
                let mut payload = audit::base_payload(&run_ctx);
                payload.insert(
                    "user_id".to_string(),
                    serde_json::Value::String(self.user_id.to_string()),
                );
                payload.insert("input".to_string(), serde_json::json!(user_message));
                payload.insert(
                    "parent_run_id".to_string(),
                    serde_json::json!(parent_run_id),
                );
                audit::event_from_map("run.started", payload)
            },
            {
                let mut payload = audit::base_payload(&run_ctx);
                payload.insert(
                    "user_id".to_string(),
                    serde_json::json!(self.user_id.to_string()),
                );
                payload.insert(
                    "bytes".to_string(),
                    serde_json::json!(full_prompt.len() as u64),
                );
                payload.insert("prompt".to_string(), serde_json::json!(full_prompt.clone()));
                audit::event_from_map("prompt.built", payload)
            },
        ];

        let llm = self.res.llm.read().clone();
        let usage_model = llm.default_model().to_string();

        let (engine_task, events, cancel, iteration) = self.spawn_engine(
            &run_id,
            &full_prompt,
            history,
            run_index,
            trace.clone(),
            &llm,
            is_remote_dispatch,
        );

        tracing::info!(
            agent_id = %self.agent_id,
            session_id = %self.id,
            run_id = %run_id,
            run_index,
            "run started"
        );
        self.mark_running(run_id.clone(), cancel, iteration);

        Ok(Stream::new(
            engine_task,
            events,
            self.state.clone(),
            self.history.clone(),
            TurnPersister::new(
                self.res.storage.clone(),
                trace,
                self.agent_id.clone(),
                self.id.clone(),
                run_id,
                self.user_id.clone(),
                start,
                self.res.recall.clone(),
            ),
            USAGE_PROVIDER_UNKNOWN.to_string(),
            usage_model,
            initial_events,
        ))
    }

    pub async fn chat(&self, user_message: &str, trace_id: &str) -> Result<Stream> {
        self.run(user_message, trace_id, None, "", "", false).await
    }

    async fn create_trace(
        &self,
        run_id: &str,
        trace_id: &str,
        parent_trace_id: &str,
        origin_node_id: &str,
    ) -> TraceRecorder {
        let effective_trace_id = if trace_id.is_empty() {
            run_id.to_string()
        } else {
            trace_id.to_string()
        };
        let mut trace = TraceRecorder::new(
            self.res.storage.trace_repo(),
            self.res.storage.span_repo(),
            effective_trace_id,
            run_id.to_string(),
            self.agent_id.to_string(),
            self.id.clone(),
            self.user_id.to_string(),
        );
        if !parent_trace_id.is_empty() {
            trace = trace.with_parent_trace(parent_trace_id, origin_node_id);
        }
        let _ = trace.start_trace("agent.run").await;
        trace
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_engine(
        &self,
        run_id: &str,
        prompt: &str,
        history: Vec<Message>,
        turn: u32,
        trace: TraceRecorder,
        llm: &Arc<dyn LLMProvider>,
        is_dispatched: bool,
    ) -> (
        JoinHandle<Result<AgentResult>>,
        mpsc::Receiver<Event>,
        CancellationToken,
        Arc<AtomicU32>,
    ) {
        let tool_view = ProgressiveToolView::new(self.res.tools.clone());
        let ctx = Context {
            agent_id: self.agent_id.clone(),
            user_id: self.user_id.clone(),
            session_id: self.id.as_str().into(),
            run_id: run_id.into(),
            turn,
            trace_id: trace.trace_id.as_str().into(),
            llm: llm.clone(),
            model: llm.default_model().into(),
            temperature: llm.default_temperature(),
            checkpoint: Arc::new(self.res.config.checkpoint.clone()),
            max_iterations: self.res.config.max_iterations,
            max_context_tokens: self.res.config.max_context_tokens,
            max_duration: Duration::from_secs(self.res.config.max_duration_secs),
            tool_view,
            system_prompt: prompt.into(),
            messages: history,
        };

        let cancel = CancellationToken::new();
        let iteration = Arc::new(AtomicU32::new(0));

        let compactor = Compactor::new(
            ctx.llm.clone(),
            ctx.model.clone(),
            ctx.checkpoint.clone(),
            cancel.clone(),
        );
        let skill_executor = Arc::new(crate::kernel::skills::runner::SkillRunner::new(
            &self.agent_id,
            &self.user_id,
            self.res.skills.clone(),
            self.res.workspace.clone(),
            self.res.storage.pool().clone(),
        ));
        let dispatcher = ToolDispatcher::new(
            self.res.tool_registry.clone(),
            skill_executor,
            ToolContext {
                user_id: self.user_id.clone(),
                session_id: self.id.as_str().into(),
                agent_id: self.agent_id.clone(),
                run_id: run_id.into(),
                trace_id: trace.trace_id.as_str().into(),
                workspace: self.res.workspace.clone(),
                pool: self.res.storage.pool().clone(),
                is_dispatched,
            },
            cancel.clone(),
        );

        let (mut engine, events) = Engine::new(
            ctx,
            dispatcher,
            compactor,
            cancel.clone(),
            iteration.clone(),
            trace,
        );
        let task = tokio::spawn(async move { engine.run().await });

        (task, events, cancel, iteration)
    }

    async fn enforce_token_limits(&self) -> Result<()> {
        let Some(record) = self.res.storage.config_get(&self.agent_id).await? else {
            return Ok(());
        };
        if let Some(total_limit) = record.token_limit_total {
            let used = self
                .res
                .storage
                .usage_summarize(UsageScope::AgentTotal {
                    agent_id: self.agent_id.to_string(),
                })
                .await?
                .total_tokens;
            if used >= total_limit {
                return Err(ErrorCode::quota_exceeded(format!(
                    "agent token total limit exceeded: used={used} limit={total_limit}"
                )));
            }
        }
        if let Some(daily_limit) = record.token_limit_daily {
            let day = crate::storage::time::now().date_naive().to_string();
            let used = self
                .res
                .storage
                .usage_summarize(UsageScope::AgentDaily {
                    agent_id: self.agent_id.to_string(),
                    day,
                })
                .await?
                .total_tokens;
            if used >= daily_limit {
                return Err(ErrorCode::quota_exceeded(format!(
                    "agent token daily limit exceeded: used={used} limit={daily_limit}"
                )));
            }
        }
        Ok(())
    }

    async fn ensure_history_loaded(&self) -> Result<()> {
        if !self.history.lock().is_empty() {
            return Ok(());
        }
        let mut seeded = Vec::new();
        let runs = self.res.storage.run_list_by_session(&self.id, 1000).await?;
        for run in runs.into_iter().rev() {
            if !run.input.is_empty() {
                seeded.push(Message::user(run.input));
            }
            if !run.output.is_empty() {
                seeded.push(Message::assistant(run.output));
            }
        }
        *self.history.lock() = seeded;
        Ok(())
    }

    fn mark_running(&self, run_id: String, cancel: CancellationToken, iteration: Arc<AtomicU32>) {
        *self.state.lock() = SessionState::Running {
            run_id,
            cancel,
            started_at: Instant::now(),
            iteration,
        };
    }

    pub fn cancel_current(&self) {
        let state = self.state.lock();
        if let SessionState::Running { cancel, .. } = &*state {
            cancel.cancel();
        }
    }

    pub fn cancel_run(&self, run_id: &str) -> bool {
        let state = self.state.lock();
        match &*state {
            SessionState::Running {
                run_id: active_run_id,
                cancel,
                ..
            } if active_run_id == run_id => {
                cancel.cancel();
                true
            }
            _ => false,
        }
    }

    pub fn set_idle(&self) {
        *self.state.lock() = SessionState::Idle;
    }

    pub fn current_run_id(&self) -> Option<String> {
        match &*self.state.lock() {
            SessionState::Running { run_id, .. } => Some(run_id.clone()),
            SessionState::Idle => None,
        }
    }

    pub async fn close(&self) {
        self.cancel_current();
        self.set_idle();
    }

    pub fn is_idle(&self) -> bool {
        matches!(*self.state.lock(), SessionState::Idle)
    }

    pub fn is_running(&self) -> bool {
        matches!(*self.state.lock(), SessionState::Running { .. })
    }

    pub fn idle_duration(&self) -> Duration {
        self.last_active.lock().elapsed()
    }

    pub fn belongs_to(&self, agent_id: &str, user_id: &str) -> bool {
        self.agent_id.as_ref() == agent_id && self.user_id.as_ref() == user_id
    }

    pub fn agent_id_ref(&self) -> &str {
        &self.agent_id
    }

    pub(crate) fn mark_stale(&self) {
        self.stale.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_stale(&self) -> bool {
        self.stale.load(Ordering::Relaxed)
    }

    pub fn info(&self) -> SessionInfo {
        let state = self.state.lock();
        let (status, current_turn) = match &*state {
            SessionState::Idle => ("idle".to_string(), None),
            SessionState::Running {
                started_at,
                iteration,
                ..
            } => (
                "running".to_string(),
                Some(TurnStats {
                    iteration: iteration.load(Ordering::Relaxed),
                    duration_ms: started_at.elapsed().as_millis() as u64,
                }),
            ),
        };
        SessionInfo {
            id: self.id.clone(),
            agent_id: self.agent_id.to_string(),
            user_id: self.user_id.to_string(),
            status,
            last_active_ms: self.last_active.lock().elapsed().as_millis() as u64,
            current_turn,
        }
    }
}
