//! Session — the aggregate root for agent conversations.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;

use super::session_manager::SessionInfo;
use super::session_manager::TurnStats;
use super::session_state::SessionState;
use crate::sessions::build::session_capabilities::SessionAssembly;
use crate::sessions::diagnostics;
use crate::sessions::runtime::run_options::RunOptions;
use crate::sessions::runtime::session_resources::SessionResources;
use crate::sessions::runtime::session_run::SessionRunCoordinator;
use crate::sessions::runtime::session_stream::Stream;
use crate::sessions::Message;
use crate::types::ErrorCode;
use crate::types::Result;

pub struct Session {
    pub id: String,
    agent_id: Arc<str>,
    user_id: Arc<str>,
    res: SessionResources,
    pub state: Arc<Mutex<SessionState>>,
    history: Arc<Mutex<Vec<Message>>>,
    last_active: Mutex<Instant>,
    stale: AtomicBool,
    queued_followup: Mutex<Option<String>>,
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
            queued_followup: Mutex::new(None),
        }
    }

    /// Create a Session from a SessionAssembly (produced by any assembler).
    /// Uses a noop pool for ephemeral sessions — DB calls won't be reached
    /// because the noop backend handles persistence.
    pub fn from_assembly(assembly: SessionAssembly) -> Self {
        let id = assembly.labels.session_id.to_string();
        let agent_id = assembly.labels.agent_id.clone();
        let user_id = assembly.labels.user_id.clone();
        let core = assembly.core;
        let infra = assembly.infra;
        let agent = assembly.agent;

        let res = SessionResources {
            workspace: core.workspace,
            toolset: core.toolset,
            org: agent.org,
            store: infra.store,
            llm: core.llm,
            config: agent.config,
            prompt_variables: agent.prompt_variables,
            cluster_client: agent.cluster_client,
            directive: agent.directive,
            tool_writer: infra.tool_writer,
            trace_writer: infra.trace_writer,
            trace_factory: infra.trace_factory,
            persist_writer: infra.persist_writer,
            prompt_config: agent.prompt_config,
            before_turn_hook: None,
            steering_source: None,
            prompt_resolver: core.prompt_resolver,
            context_provider: core.context_provider,
            run_initializer: core.run_initializer,
            skill_executor: agent.skill_executor,
        };
        Self::new(id, agent_id, user_id, res)
    }

    /// Core run entry point. Accepts full prompt metadata and run options.
    /// Invocation layer and server callers use this directly.
    pub async fn run_with_meta(
        &self,
        user_message: &str,
        meta: crate::planning::PromptRequestMeta,
        options: RunOptions,
    ) -> Result<Stream> {
        self.ensure_idle()?;
        *self.last_active.lock() = Instant::now();
        let start = Instant::now();
        SessionRunCoordinator {
            session_id: &self.id,
            agent_id: &self.agent_id,
            user_id: &self.user_id,
            resources: &self.res,
            state: &self.state,
            history: &self.history,
        }
        .start_with_options(user_message, "", None, "", "", false, start, options, meta)
        .await
    }

    /// Convenience: run with options, deriving prompt meta from overlays.
    pub async fn run_with_options(&self, user_message: &str, opts: RunOptions) -> Result<Stream> {
        let meta = crate::planning::PromptRequestMeta {
            channel_type: None,
            channel_chat_id: None,
            system_overlay: opts.system_overlay.clone(),
            skill_overlay: opts.skill_overlay.clone(),
        };
        self.run_with_meta(user_message, meta, opts).await
    }

    /// Server/router entry: run with tracing and dispatch context.
    pub async fn submit_turn(
        &self,
        user_message: &str,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
    ) -> Result<Stream> {
        self.ensure_idle()?;
        *self.last_active.lock() = Instant::now();
        let start = Instant::now();
        SessionRunCoordinator {
            session_id: &self.id,
            agent_id: &self.agent_id,
            user_id: &self.user_id,
            resources: &self.res,
            state: &self.state,
            history: &self.history,
        }
        .start_with_options(
            user_message,
            trace_id,
            parent_run_id,
            parent_trace_id,
            origin_node_id,
            is_remote_dispatch,
            start,
            RunOptions::default(),
            crate::planning::PromptRequestMeta::default(),
        )
        .await
    }

    fn ensure_idle(&self) -> Result<()> {
        let state = self.state.lock();
        if let SessionState::Running { run_id, .. } = &*state {
            diagnostics::log_run_rejected(&self.id, &self.agent_id, run_id);
            return Err(ErrorCode::denied(format!(
                "session already has a running run: {run_id}"
            )));
        }
        Ok(())
    }

    pub async fn chat(&self, user_message: &str, trace_id: &str) -> Result<Stream> {
        self.submit_turn(user_message, trace_id, None, "", "", false)
            .await
    }

    /// Inject a user message into the running engine. Returns true if sent.
    pub fn inject_message(&self, msg: &str) -> bool {
        let state = self.state.lock();
        if let SessionState::Running { inbox_tx, .. } = &*state {
            inbox_tx.try_send(Message::user(msg)).is_ok()
        } else {
            false
        }
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

    /// Clear in-memory conversation history without creating a new DB session.
    pub fn clear_history(&self) {
        self.history.lock().clear();
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

    pub fn is_stale(&self) -> bool {
        self.stale.load(Ordering::Relaxed)
    }

    pub fn queue_followup(&self, input: String) {
        let mut q = self.queued_followup.lock();
        *q = Some(match q.take() {
            Some(existing) if !existing.trim().is_empty() => {
                format!("{existing}\n\n{}", input.trim())
            }
            _ => input,
        });
    }

    pub fn take_followup(&self) -> Option<String> {
        self.queued_followup.lock().take()
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
