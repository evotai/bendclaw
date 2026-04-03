use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::run_options::RunOptions;
use super::session_resources::SessionResources;
use super::session_stream::Stream;
use crate::execution::launcher;
use crate::execution::launcher::EngineHandle;
use crate::execution::persist::persister::TurnPersister;
use crate::llm::provider::LLMProvider;
use crate::observability::audit;
use crate::observability::server_log;
use crate::planning::RunConfig;
use crate::planning::RunDeps;
use crate::planning::RunRequest;
use crate::sessions::core::session_state::SessionState;
use crate::sessions::diagnostics;
use crate::sessions::Message;
use crate::traces::TraceRecorder;
use crate::types::Result;

const USAGE_PROVIDER_UNKNOWN: &str = "unknown";

pub(in crate::sessions) struct SessionRunCoordinator<'a> {
    pub(in crate::sessions) session_id: &'a str,
    pub(in crate::sessions) agent_id: &'a Arc<str>,
    pub(in crate::sessions) user_id: &'a Arc<str>,
    pub(in crate::sessions) resources: &'a SessionResources,
    pub(in crate::sessions) state: &'a Arc<Mutex<SessionState>>,
    pub(in crate::sessions) history: &'a Arc<Mutex<Vec<Message>>>,
}

impl<'a> SessionRunCoordinator<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::sessions) async fn start_with_options(
        &self,
        user_message: &str,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
        started_at: Instant,
        opts: RunOptions,
        meta: crate::planning::PromptRequestMeta,
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
            self.resources.toolset.tools.len(),
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
                self.resources.toolset.tools.as_ref(),
                &self.resources.org.list_skills(self.user_id),
                self.user_id,
            ),
        ];

        let llm = self.resources.llm.read().clone();
        let usage_model = llm.default_model().to_string();

        let handle = self.spawn_engine(
            &run_id,
            &full_prompt,
            history,
            run_index,
            trace.clone(),
            &llm,
            is_remote_dispatch,
            &opts,
        );

        let run_persister: Arc<dyn crate::sessions::backend::sink::RunPersister> =
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

        self.mark_running(
            run_id.clone(),
            handle.cancel,
            handle.iteration,
            handle.inbox_tx,
        );

        Ok(Stream::new(
            handle.task,
            handle.events,
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
        opts: &RunOptions,
    ) -> EngineHandle {
        let deps = RunDeps::from_resources(self.resources);
        let request = RunRequest {
            user_id: self.user_id.clone(),
            agent_id: self.agent_id.clone(),
            session_id: self.session_id.into(),
            run_id: run_id.to_string(),
            turn,
            messages: history,
            system_prompt: prompt.into(),
            is_dispatched,
        };
        let config = RunConfig {
            max_iterations: opts
                .max_iterations
                .unwrap_or(self.resources.config.max_iterations),
            max_context_tokens: self.resources.config.max_context_tokens,
            max_duration: Duration::from_secs(
                opts.max_duration_secs
                    .unwrap_or(self.resources.config.max_duration_secs),
            ),
            llm: llm.clone(),
        };
        launcher::launch(deps, trace, request, config)
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
