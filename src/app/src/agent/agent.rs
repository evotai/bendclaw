use std::path::PathBuf;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    Normal,
    Planning,
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
// AppAgent
// ---------------------------------------------------------------------------

const PLANNING_MODE_PROMPT: &str = "\
You are in planning mode — read-only tools only, no edits or changes allowed.

Pair-plan with the user iteratively:
1. **Explore** — read code with the available tools, look for reusable patterns
2. **Summarize** — capture findings immediately, don't wait
3. **Ask** — when you hit ambiguity only the user can resolve, ask directly

Start by scanning key files for a quick overview, then outline a skeleton plan \
and ask your first questions. Don't explore exhaustively before engaging the user.

When presenting a plan, structure it as:
- **Context** — why this change is needed
- **Approach** — recommended implementation
- **Files** — paths to modify, existing code to reuse
- **Verification** — how to test

The user will use /act when ready to implement.";

/// Read-only tools for planning/investigation mode.
fn read_only_tools() -> Vec<Box<dyn bend_engine::AgentTool>> {
    use bend_engine::tools::*;
    vec![
        Box::new(ReadFileTool::default()),
        Box::new(ListFilesTool::default()),
        Box::new(SearchTool::default()),
        Box::new(WebFetchTool::new()),
    ]
}

pub struct AppAgent {
    llm: RwLock<LlmConfig>,
    system_prompt: RwLock<String>,
    limits: RwLock<ExecutionLimits>,
    skills_dirs: RwLock<Vec<PathBuf>>,
    tool_mode: RwLock<ToolMode>,
    cwd: String,
    storage: RwLock<Arc<dyn Storage>>,
    engine: RwLock<Option<EngineHandle>>,
}

impl AppAgent {
    pub fn new(config: &Config, cwd: impl Into<String>) -> Result<Arc<Self>> {
        let cwd = cwd.into();
        let storage = open_storage(&config.storage)?;
        let system_prompt = format!("You are a helpful assistant. Working directory: {cwd}");
        Ok(Arc::new(Self {
            llm: RwLock::new(config.active_llm()),
            system_prompt: RwLock::new(system_prompt),
            limits: RwLock::new(ExecutionLimits::default()),
            skills_dirs: RwLock::new(Vec::new()),
            tool_mode: RwLock::new(ToolMode::Normal),
            cwd,
            storage: RwLock::new(storage),
            engine: RwLock::new(None),
        }))
    }

    pub fn with_system_prompt(self: &Arc<Self>, prompt: impl Into<String>) -> Arc<Self> {
        *self.system_prompt.write() = prompt.into();
        Arc::clone(self)
    }

    pub fn append_system_prompt(self: &Arc<Self>, extra: &str) -> Arc<Self> {
        let mut sp = self.system_prompt.write();
        sp.push('\n');
        sp.push_str(extra);
        drop(sp);
        Arc::clone(self)
    }

    pub fn with_limits(self: &Arc<Self>, limits: ExecutionLimits) -> Arc<Self> {
        *self.limits.write() = limits;
        Arc::clone(self)
    }

    pub fn with_skills_dirs(self: &Arc<Self>, dirs: Vec<PathBuf>) -> Arc<Self> {
        *self.skills_dirs.write() = dirs;
        Arc::clone(self)
    }

    pub fn with_tool_mode(self: &Arc<Self>, mode: ToolMode) -> Arc<Self> {
        *self.tool_mode.write() = mode;
        Arc::clone(self)
    }

    pub fn with_storage(self: &Arc<Self>, storage: Arc<dyn Storage>) -> Arc<Self> {
        *self.storage.write() = storage;
        Arc::clone(self)
    }

    pub fn system_prompt(&self) -> String {
        self.system_prompt.read().clone()
    }

    pub fn llm(&self) -> LlmConfig {
        self.llm.read().clone()
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn limits(&self) -> ExecutionLimits {
        self.limits.read().clone()
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

    pub fn tool_mode(&self) -> ToolMode {
        *self.tool_mode.read()
    }

    pub fn effective_prompt(&self, input: &str) -> String {
        match *self.tool_mode.read() {
            ToolMode::Normal => input.to_string(),
            ToolMode::Planning => format!("{}\n\nUser task:\n{}", PLANNING_MODE_PROMPT, input),
        }
    }

    // -- core: run a turn, return a stream of RunEvents --------------------

    pub async fn run(&self, request: TurnRequest) -> Result<TurnStream> {
        let session = self.open_session(request.session_id.as_deref()).await?;
        let session_meta = session.meta().await;
        let session_id = session_meta.session_id.clone();
        let run_id = crate::ids::new_id();
        let llm = self.llm.read().clone();
        let model = llm.model.clone();

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
            let mut saved_count: usize = 0;
            let mut turn = 0_u32;
            let mut got_agent_end = false;
            let mut engine_rx = engine_rx;

            // Flush unsaved transcript items to storage.
            let flush =
                |session: &Arc<Session>, transcripts: &[TranscriptItem], saved: &mut usize| {
                    let new_items = transcripts[*saved..].to_vec();
                    let session = Arc::clone(session);
                    *saved = transcripts.len();
                    async move {
                        if !new_items.is_empty() {
                            session.write_items(new_items).await
                        } else {
                            Ok(())
                        }
                    }
                };

            while let Some(protocol_event) = engine_rx.recv().await {
                if matches!(protocol_event, ProtocolEvent::TurnStart) {
                    turn += 1;
                    session.increment_turn().await;
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

                        // Emit an Error RunEvent when the LLM turn ended with an error
                        if stop_reason == "error" {
                            let err_msg = error_message
                                .clone()
                                .unwrap_or_else(|| "Unknown error".to_string());
                            let error_event = RunEventContext::new(&rid, &sid, turn)
                                .map(&ProtocolEvent::InputRejected { reason: err_msg });
                            if let Some(evt) = error_event {
                                let _ = tx.send(evt);
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
                    ProtocolEvent::ContextCompactionEnd {
                        ref compacted_transcripts,
                        level,
                        ..
                    } => {
                        if *level > 0 {
                            run_transcripts.push(TranscriptItem::Compact {
                                messages: compacted_transcripts.clone(),
                            });
                        }
                    }
                    ProtocolEvent::TurnEnd => {
                        if let Err(e) = flush(&session, &run_transcripts, &mut saved_count).await {
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

                    if let Err(e) = flush(&session, &run_transcripts, &mut saved_count).await {
                        logx!(
                            error,
                            "run",
                            "transcript_save_failed",
                            run_id = %rid,
                            session_id = %sid,
                            error = %e,
                        );
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
                    let _ = tx.send(finished_event);
                    continue;
                }

                let event_context = RunEventContext::new(&rid, &sid, turn);
                if let Some(event) = event_context.map(&protocol_event) {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
            }

            // Fallback save
            if !got_agent_end {
                let _ = flush(&session, &run_transcripts, &mut saved_count).await;
            }

            let _ = session.save().await;

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
        let engine = self.engine.read();
        if let Some(h) = engine.as_ref() {
            h.abort();
        }
    }

    // -- session queries (for REPL / Server UI) ----------------------------

    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<SessionMeta>> {
        let storage = self.storage.read().clone();
        storage.list_sessions(ListSessions { limit }).await
    }

    pub async fn get_session(&self, id: &str) -> Result<Option<SessionMeta>> {
        let storage = self.storage.read().clone();
        storage.get_session(id).await
    }

    pub async fn load_transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        let storage = self.storage.read().clone();
        match Session::open(id, storage).await? {
            Some(session) => Ok(session.transcript().await),
            None => Ok(Vec::new()),
        }
    }

    pub async fn load_session(&self, id: &str) -> Result<Option<Arc<Session>>> {
        let storage = self.storage.read().clone();
        Session::open(id, storage).await
    }

    pub fn storage(&self) -> Arc<dyn Storage> {
        self.storage.read().clone()
    }

    // -- private -----------------------------------------------------------

    async fn open_session(&self, session_id: Option<&str>) -> Result<Arc<Session>> {
        let model = self.llm.read().model.clone();
        let storage = self.storage.read().clone();
        match session_id {
            Some(id) => match Session::open(id, storage).await? {
                Some(session) => {
                    session.set_model(model).await;
                    Ok(session)
                }
                None => Err(BendclawError::Session(format!("session not found: {id}"))),
            },
            None => {
                let id = crate::ids::new_id();
                Session::new(id, self.cwd.clone(), model, storage).await
            }
        }
    }

    async fn start_engine(
        &self,
        prompt: String,
        prior_transcripts: &[TranscriptItem],
    ) -> Result<mpsc::UnboundedReceiver<ProtocolEvent>> {
        let llm = self.llm.read().clone();
        let tools = match *self.tool_mode.read() {
            ToolMode::Planning => read_only_tools(),
            ToolMode::Normal => bend_engine::tools::base_tools(),
        };
        let options = EngineOptions {
            provider: llm.provider,
            model: llm.model,
            api_key: llm.api_key,
            base_url: llm.base_url,
            system_prompt: self.system_prompt.read().clone(),
            limits: self.limits.read().clone(),
            skills_dirs: self.skills_dirs.read().clone(),
            tools,
        };
        let (rx, engine_handle) =
            crate::protocol::engine::start_engine(options, prior_transcripts, prompt).await?;

        *self.engine.write() = Some(engine_handle);
        Ok(rx)
    }
}
