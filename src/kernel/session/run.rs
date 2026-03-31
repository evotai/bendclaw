use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::diagnostics;
use super::options::RunOptions;
use super::resources::SessionResources;
use super::state::SessionState;
use crate::base::Result;
use crate::kernel::execution::CallExecutor;
use crate::kernel::run::compaction::Compactor;
use crate::kernel::run::context::Context;
use crate::kernel::run::engine::Engine;
use crate::kernel::run::event::Event;
use crate::kernel::run::persister::TurnPersister;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::session::session_stream::Stream;
use crate::kernel::tools::progressive::ProgressiveToolView;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolRuntime;
use crate::kernel::trace::TraceRecorder;
use crate::kernel::Message;
use crate::llm::provider::LLMProvider;
use crate::observability::audit;
use crate::observability::server_log;

const USAGE_PROVIDER_UNKNOWN: &str = "unknown";

pub(super) struct SessionRunCoordinator<'a> {
    pub(super) session_id: &'a str,
    pub(super) agent_id: &'a Arc<str>,
    pub(super) user_id: &'a Arc<str>,
    pub(super) resources: &'a SessionResources,
    pub(super) state: &'a Arc<Mutex<SessionState>>,
    pub(super) history: &'a Arc<Mutex<Vec<Message>>>,
}

impl<'a> SessionRunCoordinator<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn start_with_options(
        &self,
        user_message: &str,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
        started_at: Instant,
        opts: RunOptions,
        meta: crate::kernel::run::prompt::PromptRequestMeta,
    ) -> Result<Stream> {
        self.enforce_token_limits().await?;

        self.ensure_history_loaded().await?;
        let mut history = self.history.lock().clone();
        let prior_history = history.clone();
        let run_index = history
            .iter()
            .filter(|m| matches!(m, Message::User { .. }))
            .count() as u32
            + 1;

        let run_id = self.resources.run_initializer.init_run(
            user_message,
            parent_run_id,
            &self.resources.config.node_id,
        )?;

        history.push(Message::user(user_message).with_run_id(run_id.clone()));
        let context_preview = diagnostics::ContextPreview::from_history(
            &prior_history,
            &history,
            user_message,
            &run_id,
        );

        let trace = self
            .create_trace(&run_id, trace_id, parent_trace_id, origin_node_id)
            .await;
        let run_ctx = server_log::ServerCtx::new(
            &trace.trace_id,
            &run_id,
            self.session_id,
            self.agent_id,
            run_index,
        );
        diagnostics::log_run_started(
            run_ctx,
            self.user_id,
            user_message,
            run_index,
            parent_run_id,
        );

        diagnostics::log_context_prepared(
            run_ctx,
            user_message,
            run_index,
            &history,
            &context_preview,
        );

        let full_prompt = self.resources.prompt_resolver.resolve(&meta).await?;

        diagnostics::log_prompt_built(
            run_ctx,
            self.user_id,
            full_prompt.len(),
            self.resources.tools.len(),
            history.len(),
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
            crate::kernel::workbench::sem_event::capture_capabilities(
                self.resources.tools.as_ref(),
                &self.resources.org.list_skills(self.user_id),
                self.user_id,
            ),
        ];

        let llm = self.resources.llm.read().clone();
        let usage_model = llm.default_model().to_string();

        let (engine_task, events, cancel, iteration, inbox_tx) = self.spawn_engine(
            &run_id,
            &full_prompt,
            history,
            run_index,
            trace.clone(),
            &llm,
            is_remote_dispatch,
            &opts,
        );

        let run_persister: Arc<dyn crate::kernel::session::backend::sink::RunPersister> =
            Arc::new(TurnPersister::new(
                self.resources.store.clone(),
                trace,
                self.agent_id.clone(),
                self.session_id.to_string(),
                &run_id,
                self.user_id.clone(),
                started_at,
                self.resources.persist_writer.clone(),
                llm.clone(),
            ));

        self.mark_running(run_id.clone(), cancel, iteration, inbox_tx);

        Ok(Stream::new(
            engine_task,
            events,
            self.state.clone(),
            self.history.clone(),
            run_persister,
            run_id,
            USAGE_PROVIDER_UNKNOWN.to_string(),
            usage_model,
            initial_events,
        ))
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
        let mut trace = self.resources.trace_factory.create_recorder(
            &self.resources.trace_writer,
            effective_trace_id,
            run_id.to_string(),
            self.agent_id.to_string(),
            self.session_id.to_string(),
            self.user_id.to_string(),
        );
        if !parent_trace_id.is_empty() {
            trace = trace.with_parent_trace(parent_trace_id, origin_node_id);
        }
        trace.start_trace("agent.run");
        trace
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn spawn_engine(
        &self,
        run_id: &str,
        prompt: &str,
        history: Vec<Message>,
        turn: u32,
        trace: TraceRecorder,
        llm: &Arc<dyn LLMProvider>,
        is_dispatched: bool,
        opts: &RunOptions,
    ) -> (
        JoinHandle<Result<AgentResult>>,
        mpsc::Receiver<Event>,
        CancellationToken,
        Arc<AtomicU32>,
        mpsc::Sender<Message>,
    ) {
        let tool_view = ProgressiveToolView::new(self.resources.tools.clone());
        let ctx = Context {
            agent_id: self.agent_id.clone(),
            user_id: self.user_id.clone(),
            session_id: self.session_id.into(),
            run_id: run_id.into(),
            turn,
            trace_id: trace.trace_id.as_str().into(),
            llm: llm.clone(),
            model: llm.default_model().into(),
            temperature: llm.default_temperature(),
            max_iterations: opts
                .max_iterations
                .unwrap_or(self.resources.config.max_iterations),
            max_context_tokens: self.resources.config.max_context_tokens,
            max_duration: Duration::from_secs(
                opts.max_duration_secs
                    .unwrap_or(self.resources.config.max_duration_secs),
            ),
            tool_view,
            system_prompt: prompt.into(),
            messages: history,
        };

        let cancel = CancellationToken::new();
        let iteration = Arc::new(AtomicU32::new(0));

        let compactor = Compactor::new(ctx.llm.clone(), ctx.model.clone(), cancel.clone());
        let skill_executor = self.resources.skill_executor.clone();
        let (tx, rx) = Engine::create_channel();
        let event_tx = tx.clone();
        let executor = CallExecutor::new(
            self.resources.tool_registry.clone(),
            skill_executor,
            ToolContext {
                user_id: self.user_id.clone(),
                session_id: self.session_id.into(),
                agent_id: self.agent_id.clone(),
                run_id: run_id.into(),
                trace_id: trace.trace_id.as_str().into(),
                workspace: self.resources.workspace.clone(),
                is_dispatched,
                runtime: ToolRuntime {
                    event_tx: None,
                    cancel: cancel.clone(),
                    tool_call_id: None,
                },
                tool_writer: self.resources.tool_writer.clone(),
            },
            cancel.clone(),
            event_tx,
        )
        .with_allowed_tool_names(self.resources.allowed_tool_names.clone());

        let (inbox_tx, inbox_rx) = Engine::create_inbox();

        let extract_memory = self
            .resources
            .org
            .memory()
            .filter(|_| self.resources.config.memory.extract);
        let mut engine = Engine::from_tx(
            ctx,
            executor,
            compactor,
            cancel.clone(),
            iteration.clone(),
            trace,
            tx,
            inbox_rx,
            extract_memory,
        );
        if let Some(ref hook) = self.resources.before_turn_hook {
            engine = engine.with_before_turn(Box::new(Arc::clone(hook)));
        }
        if let Some(ref source) = self.resources.steering_source {
            engine = engine.with_steering(Box::new(Arc::clone(source)));
        }
        let events = rx;
        let task = tokio::spawn(async move { engine.run().await });

        (task, events, cancel, iteration, inbox_tx)
    }

    async fn enforce_token_limits(&self) -> Result<()> {
        self.resources.context_provider.enforce_token_limits().await
    }

    async fn ensure_history_loaded(&self) -> Result<()> {
        if !self.history.lock().is_empty() {
            return Ok(());
        }
        let loaded = self.resources.context_provider.load_history(1000).await?;
        *self.history.lock() = loaded;
        Ok(())
    }

    fn mark_running(
        &self,
        run_id: String,
        cancel: CancellationToken,
        iteration: Arc<AtomicU32>,
        inbox_tx: mpsc::Sender<Message>,
    ) {
        *self.state.lock() = SessionState::Running {
            run_id,
            cancel,
            started_at: Instant::now(),
            iteration,
            inbox_tx,
        };
    }
}
