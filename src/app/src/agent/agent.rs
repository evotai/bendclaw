use std::sync::Arc;
use std::time::Instant;

use bend_base::logx;
use parking_lot::RwLock;
use tokio::sync::mpsc;

use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::protocol::engine::EngineHandle;
use crate::protocol::engine::EngineOptions;
use crate::protocol::ListSessions;
use crate::protocol::ProtocolEvent;
use crate::protocol::RunEvent;
use crate::protocol::RunEventContext;
use crate::protocol::RunMeta;
use crate::protocol::RunStatus;
use crate::protocol::SessionMeta;
use crate::protocol::TranscriptItem;
use crate::session::Session;
use crate::storage::open_storage;
use crate::storage::Storage;

// ---------------------------------------------------------------------------
// ExecutionLimits
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ExecutionLimits {
    pub max_turns: u32,
    pub max_total_tokens: u64,
    pub max_duration_secs: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 512,
            max_total_tokens: 100_000_000,
            max_duration_secs: 3600,
        }
    }
}

// ---------------------------------------------------------------------------
// TurnRequest
// ---------------------------------------------------------------------------

pub struct TurnRequest {
    pub prompt: String,
    pub session_id: Option<String>,
}

impl TurnRequest {
    pub fn text(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            session_id: None,
        }
    }

    pub fn session_id(mut self, id: Option<String>) -> Self {
        self.session_id = id;
        self
    }
}

// ---------------------------------------------------------------------------
// TurnStream
// ---------------------------------------------------------------------------

pub struct TurnStream {
    rx: mpsc::UnboundedReceiver<RunEvent>,
    pub session_id: String,
    pub run_id: String,
}

impl TurnStream {
    pub async fn next(&mut self) -> Option<RunEvent> {
        self.rx.recv().await
    }
}

// ---------------------------------------------------------------------------
// AgentState (internal)
// ---------------------------------------------------------------------------

enum AgentState {
    Live { handle: Box<Option<EngineHandle>> },
    Scripted { events: Vec<ProtocolEvent> },
}

// ---------------------------------------------------------------------------
// AppAgent
// ---------------------------------------------------------------------------

pub struct AppAgent {
    llm: RwLock<LlmConfig>,
    system_prompt: String,
    limits: ExecutionLimits,
    cwd: String,
    storage: Arc<dyn Storage>,
    state: RwLock<AgentState>,
}

impl AppAgent {
    pub fn new(config: &Config, cwd: impl Into<String>) -> Result<Self> {
        let cwd = cwd.into();
        let storage = open_storage(&config.storage)?;
        let system_prompt = format!("You are a helpful assistant. Working directory: {cwd}");
        Ok(Self {
            llm: RwLock::new(config.active_llm()),
            system_prompt,
            limits: ExecutionLimits::default(),
            cwd,
            storage,
            state: RwLock::new(AgentState::Live {
                handle: Box::new(None),
            }),
        })
    }

    pub fn scripted(
        events: Vec<ProtocolEvent>,
        _transcripts: Vec<TranscriptItem>,
        storage: Arc<dyn Storage>,
    ) -> Arc<Self> {
        Arc::new(Self {
            llm: RwLock::new(LlmConfig {
                provider: crate::conf::ProviderKind::Anthropic,
                api_key: String::new(),
                base_url: None,
                model: String::new(),
            }),
            system_prompt: String::new(),
            limits: ExecutionLimits::default(),
            cwd: String::new(),
            storage,
            state: RwLock::new(AgentState::Scripted { events }),
        })
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn append_system_prompt(mut self, extra: &str) -> Self {
        self.system_prompt.push('\n');
        self.system_prompt.push_str(extra);
        self
    }

    pub fn with_limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_storage(mut self, storage: Arc<dyn Storage>) -> Self {
        self.storage = storage;
        self
    }

    pub fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    pub fn llm(&self) -> LlmConfig {
        self.llm.read().clone()
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn limits(&self) -> &ExecutionLimits {
        &self.limits
    }

    pub fn set_model(&self, model: String) {
        self.llm.write().model = model;
    }

    pub fn set_provider(&self, provider: crate::conf::ProviderKind) {
        self.llm.write().provider = provider;
    }

    pub fn set_llm(&self, llm: LlmConfig) {
        *self.llm.write() = llm;
    }

    // -- core: run a turn, return a stream of RunEvents --------------------

    pub async fn run(&self, request: TurnRequest) -> Result<TurnStream> {
        let session = self.open_session(request.session_id.as_deref()).await?;
        let session_meta = session.meta().await;
        let session_id = session_meta.session_id.clone();
        let run_id = crate::ids::new_id();
        let llm = self.llm.read().clone();
        let model = llm.model.clone();

        let mut run_meta = RunMeta::new(run_id.clone(), session_id.clone(), model.clone());
        self.storage.put_run(run_meta.clone()).await?;

        logx!(
            info,
            "run",
            "started",
            run_id = %run_id,
            session_id = %session_id,
            provider = ?llm.provider,
            model = %model,
        );

        let prior_transcripts = session.transcript().await;
        let engine_rx = self
            .start_engine(request.prompt.clone(), &prior_transcripts)
            .await?;

        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn background task: consume engine events → persist → forward RunEvents
        let storage = self.storage.clone();
        let prompt = request.prompt;
        let rid = run_id.clone();
        let sid = session_id.clone();
        tokio::spawn(async move {
            let started_at = Instant::now();
            let ctx = RunEventContext::new(&rid, &sid, 0);

            // Send started event
            let started_event = ctx.started();
            let _ = tx.send(started_event.clone());

            let mut run_transcripts: Vec<TranscriptItem> =
                vec![TranscriptItem::User { text: prompt }];
            let mut run_events: Vec<RunEvent> = vec![started_event];
            let mut turn = 0_u32;
            let mut got_agent_end = false;
            let mut got_assistant_response = false;
            let mut engine_rx = engine_rx;

            while let Some(protocol_event) = engine_rx.recv().await {
                if matches!(protocol_event, ProtocolEvent::TurnStart) {
                    turn += 1;
                }

                // Incrementally build transcript
                match &protocol_event {
                    ProtocolEvent::AssistantCompleted {
                        content,
                        stop_reason,
                        error_message,
                        ..
                    } => {
                        let item =
                            crate::protocol::engine::transcript::transcript_from_assistant_completed(
                                content,
                                stop_reason,
                            );
                        run_transcripts.push(item);
                        got_assistant_response = true;

                        // Emit an Error RunEvent when the LLM turn ended with an error
                        if stop_reason == "error" {
                            let err_msg = error_message
                                .clone()
                                .unwrap_or_else(|| "Unknown error".to_string());
                            let error_event = RunEventContext::new(&rid, &sid, turn)
                                .map(&ProtocolEvent::InputRejected { reason: err_msg });
                            if let Some(evt) = error_event {
                                let _ = tx.send(evt.clone());
                                run_events.push(evt);
                            }
                        }
                    }
                    ProtocolEvent::ToolEnd {
                        tool_call_id,
                        tool_name,
                        content,
                        is_error,
                        ..
                    } => {
                        run_transcripts.push(TranscriptItem::ToolResult {
                            tool_call_id: tool_call_id.clone(),
                            tool_name: tool_name.clone(),
                            content: content.clone(),
                            is_error: *is_error,
                        });
                    }
                    ProtocolEvent::TurnEnd => {
                        let full: Vec<TranscriptItem> = prior_transcripts
                            .iter()
                            .chain(run_transcripts.iter())
                            .cloned()
                            .collect();
                        if let Err(e) = session.apply_and_save(full).await {
                            logx!(
                                error,
                                "run",
                                "incremental_save_failed",
                                run_id = %rid,
                                session_id = %sid,
                                error = %e,
                            );
                        }
                    }
                    _ => {}
                }

                if let ProtocolEvent::AgentEnd {
                    ref transcripts,
                    ref usage,
                    transcript_count,
                } = protocol_event
                {
                    got_agent_end = true;

                    if !transcripts.is_empty() {
                        if let Err(e) = session.apply_and_save(transcripts.clone()).await {
                            logx!(
                                error,
                                "run",
                                "transcript_save_failed",
                                run_id = %rid,
                                session_id = %sid,
                                error = %e,
                            );
                        }
                    }

                    let last_text = transcripts
                        .iter()
                        .rev()
                        .find_map(|t| {
                            if let TranscriptItem::Assistant { text, .. } = t {
                                if !text.is_empty() {
                                    return Some(text.clone());
                                }
                            }
                            None
                        })
                        .unwrap_or_default();

                    let finished_event = RunEventContext::new(&rid, &sid, turn).finished(
                        last_text,
                        usage.clone(),
                        turn,
                        started_at.elapsed().as_millis() as u64,
                        transcript_count,
                    );
                    let _ = tx.send(finished_event.clone());
                    run_events.push(finished_event);
                    continue;
                }

                let event_context = RunEventContext::new(&rid, &sid, turn);
                if let Some(event) = event_context.map(&protocol_event) {
                    if tx.send(event.clone()).is_err() {
                        break;
                    }
                    run_events.push(event);
                }
            }

            // Fallback save
            if !got_agent_end && run_transcripts.len() > 1 {
                let full: Vec<TranscriptItem> = prior_transcripts
                    .iter()
                    .chain(run_transcripts.iter())
                    .cloned()
                    .collect();
                let _ = session.apply_and_save(full).await;
            }

            let _ = session.save().await;

            if got_agent_end && got_assistant_response {
                run_meta.finish(RunStatus::Completed);
            } else {
                run_meta.finish(RunStatus::Failed);
            }

            let _ = storage.put_run(run_meta).await;
            let _ = storage.put_run_events(run_events).await;

            logx!(
                info,
                "run",
                "finished",
                run_id = %rid,
                session_id = %sid,
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                turn,
            );
        });

        Ok(TurnStream {
            rx,
            session_id,
            run_id,
        })
    }

    pub fn abort(&self) {
        let state = self.state.read();
        match &*state {
            AgentState::Live { handle } => {
                if let Some(h) = handle.as_ref() {
                    h.abort();
                }
            }
            AgentState::Scripted { .. } => {}
        }
    }

    // -- session queries (for REPL / Server UI) ----------------------------

    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<SessionMeta>> {
        self.storage.list_sessions(ListSessions { limit }).await
    }

    pub async fn get_session(&self, id: &str) -> Result<Option<SessionMeta>> {
        self.storage.get_session(id).await
    }

    pub async fn load_transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        match Session::load(id, self.storage.clone()).await? {
            Some(session) => Ok(session.transcript().await),
            None => Ok(Vec::new()),
        }
    }

    pub async fn load_session(&self, id: &str) -> Result<Option<Arc<Session>>> {
        Session::load(id, self.storage.clone()).await
    }

    pub fn storage(&self) -> &Arc<dyn Storage> {
        &self.storage
    }

    // -- private -----------------------------------------------------------

    async fn open_session(&self, session_id: Option<&str>) -> Result<Arc<Session>> {
        let model = self.llm.read().model.clone();
        match session_id {
            Some(id) => match Session::load(id, self.storage.clone()).await? {
                Some(session) => {
                    session.set_model(model).await;
                    Ok(session)
                }
                None => Err(BendclawError::Session(format!("session not found: {id}"))),
            },
            None => {
                let id = crate::ids::new_id();
                Session::create(id, self.cwd.clone(), model, self.storage.clone()).await
            }
        }
    }

    async fn start_engine(
        &self,
        prompt: String,
        prior_transcripts: &[TranscriptItem],
    ) -> Result<mpsc::UnboundedReceiver<ProtocolEvent>> {
        // Check state under a short lock, then do async work outside the lock
        let is_scripted = {
            let state = self.state.read();
            matches!(&*state, AgentState::Scripted { .. })
        };

        if is_scripted {
            let events = {
                let state = self.state.read();
                match &*state {
                    AgentState::Scripted { events } => events.clone(),
                    _ => Vec::new(),
                }
            };
            let (tx, rx) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                for event in events {
                    let _ = tx.send(event);
                }
            });
            return Ok(rx);
        }

        // Live path: build options, then start engine, then store handle
        let llm = self.llm.read().clone();
        let options = EngineOptions {
            provider: llm.provider,
            model: llm.model,
            api_key: llm.api_key,
            base_url: llm.base_url,
            system_prompt: self.system_prompt.clone(),
            limits: self.limits.clone(),
        };
        let (rx, engine_handle) =
            crate::protocol::engine::start_engine(&options, prior_transcripts, prompt).await?;

        {
            let mut state = self.state.write();
            if let AgentState::Live { handle } = &mut *state {
                **handle = Some(engine_handle);
            }
        }
        Ok(rx)
    }
}
