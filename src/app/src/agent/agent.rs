use std::path::PathBuf;
use std::sync::Arc;

use bend_base::logx;
use parking_lot::RwLock;
use tokio::sync::mpsc;

use super::event::RunEvent;
use super::runtime::EngineHandle;
use super::runtime::EngineOptions;
use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::session::Session;
use crate::storage::open_storage;
use crate::storage::Storage;
use crate::types::ListSessions;
use crate::types::SessionMeta;
use crate::types::TranscriptItem;

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
    engine_handle: EngineHandle,
}

impl TurnStream {
    pub async fn next(&mut self) -> Option<RunEvent> {
        self.rx.recv().await
    }

    /// Abort the current run.
    pub fn abort(&self) {
        self.engine_handle.abort();
    }
}

// ---------------------------------------------------------------------------
// AppAgent
// ---------------------------------------------------------------------------

const PLANNING_MODE_PROMPT: &str = include_str!("prompts/plan.md");

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
        }))
    }

    // -- configuration (fluent setters) --------------------------------------

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

    // -- getters -------------------------------------------------------------

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

    // -- core: submit a turn, return a stream of RunEvents -------------------

    pub async fn submit(&self, request: TurnRequest) -> Result<TurnStream> {
        let session = self.resolve_session(request.session_id.as_deref()).await?;
        let session_meta = session.meta().await;
        let session_id = session_meta.session_id.clone();
        let run_id = crate::types::new_id();
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

        let prompt = self.build_prompt(&request.prompt);
        let prior_transcripts = session.transcript().await;
        let (runtime_rx, engine_handle) = self
            .create_engine(&prompt, &prior_transcripts, &run_id, &session_id)
            .await?;

        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(super::runtime::run_loop(
            runtime_rx,
            tx,
            session,
            prompt,
            run_id.clone(),
            session_id.clone(),
        ));

        Ok(TurnStream {
            rx,
            session_id,
            run_id,
            engine_handle,
        })
    }

    // -- session queries -----------------------------------------------------

    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<SessionMeta>> {
        let storage = self.storage.read().clone();
        storage.list_sessions(ListSessions { limit }).await
    }

    pub async fn find_session(&self, id: &str) -> Result<Option<SessionMeta>> {
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

    // -- private -------------------------------------------------------------

    fn build_prompt(&self, input: &str) -> String {
        match *self.tool_mode.read() {
            ToolMode::Normal => input.to_string(),
            ToolMode::Planning => format!("{}\n\nUser task:\n{}", PLANNING_MODE_PROMPT, input),
        }
    }

    async fn resolve_session(&self, session_id: Option<&str>) -> Result<Arc<Session>> {
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
                let id = crate::types::new_id();
                Session::new(id, self.cwd.clone(), model, storage).await
            }
        }
    }

    async fn create_engine(
        &self,
        prompt: &str,
        prior_transcripts: &[TranscriptItem],
        run_id: &str,
        session_id: &str,
    ) -> Result<(
        mpsc::UnboundedReceiver<super::runtime::RuntimeEvent>,
        EngineHandle,
    )> {
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
        super::runtime::create_engine(
            options,
            prior_transcripts,
            prompt.to_string(),
            run_id,
            session_id,
        )
        .await
    }
}
