use std::path::PathBuf;
use std::sync::Arc;

use bend_base::logx;
use parking_lot::RwLock;
use tokio::sync::mpsc;

use super::event::RunEvent;
use super::runtime::EngineHandle;
use super::runtime::EngineOptions;
use super::variables::Variables;
use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::session::Session;
use crate::storage::open_storage;
use crate::storage::MemoryStorage;
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
    pub ask_fn: Option<bend_engine::tools::AskUserFn>,
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
            ask_fn: None,
        }
    }

    pub fn session_id(mut self, id: Option<String>) -> Self {
        self.session_id = id;
        self
    }

    pub fn ask_fn(mut self, f: bend_engine::tools::AskUserFn) -> Self {
        self.ask_fn = Some(f);
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

const PLANNING_MODE_PROMPT: &str = include_str!("prompt/plan.md");

/// Factory function that produces a tool set.
type ToolSetFactory = fn() -> Vec<Box<dyn bend_engine::AgentTool>>;

pub struct AppAgent {
    llm: RwLock<LlmConfig>,
    system_prompt: RwLock<String>,
    limits: RwLock<ExecutionLimits>,
    skills_dirs: RwLock<Vec<PathBuf>>,
    tool_mode: RwLock<ToolMode>,
    cwd: String,
    storage: RwLock<Arc<dyn Storage>>,
    variables: RwLock<Option<Arc<Variables>>>,
    /// When set, `create_engine` uses these tools instead of `tool_mode`.
    /// Used by side conversations to inject readonly tools.
    tools_override: Option<ToolSetFactory>,
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
            variables: RwLock::new(None),
            tools_override: None,
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
        self.with_claude_skills_dirs()
    }

    /// Temporary compatibility: load skills from ~/.claude/skills if it exists.
    fn with_claude_skills_dirs(self: &Arc<Self>) -> Arc<Self> {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let claude_dir = PathBuf::from(home).join(".claude").join("skills");
            if claude_dir.is_dir() {
                self.skills_dirs.write().push(claude_dir);
            }
        }
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

    pub fn with_variables(self: &Arc<Self>, variables: Arc<Variables>) -> Arc<Self> {
        *self.variables.write() = Some(variables);
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

    pub fn variables(&self) -> Option<Arc<Variables>> {
        self.variables.read().clone()
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

        let prior_transcripts = session.transcript().await;
        let (runtime_rx, engine_handle) = self
            .create_engine(
                &request.prompt,
                &prior_transcripts,
                &run_id,
                &session_id,
                request.ask_fn,
            )
            .await?;

        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(super::runtime::run_loop(
            runtime_rx,
            tx,
            session,
            request.prompt,
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

    fn build_system_prompt(&self) -> String {
        let base = self.system_prompt.read().clone();
        let mut prompt = match *self.tool_mode.read() {
            ToolMode::Normal => base,
            ToolMode::Planning => format!("{base}\n\n{PLANNING_MODE_PROMPT}"),
        };

        if let Some(vars) = self.variables.read().as_ref() {
            let names = vars.variable_names();
            if !names.is_empty() {
                prompt.push_str("\n\nAvailable variables: ");
                prompt.push_str(&names.join(", "));
                prompt.push_str(
                    "\n\nThese variables are automatically available in all bash commands \
                     as environment variables. Use $VAR_NAME to reference them.\n\
                     Do not print, echo, or expose variable values.",
                );
            }
        }

        prompt
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
        ask_fn: Option<bend_engine::tools::AskUserFn>,
    ) -> Result<(
        mpsc::UnboundedReceiver<super::runtime::RuntimeEvent>,
        EngineHandle,
    )> {
        let llm = self.llm.read().clone();
        let env_pairs = self
            .variables()
            .map(|v| v.all_env_pairs())
            .unwrap_or_default();
        let tools = if let Some(tools_fn) = self.tools_override {
            // tools_override provides a fixed tool set (e.g. readonly for side
            // conversations) — do not append extra tools like MemoryTool.
            tools_fn()
        } else {
            let mut t = match *self.tool_mode.read() {
                ToolMode::Planning => bend_engine::tools::planning_tools(
                    ask_fn,
                    "This tool is not allowed in planning mode. \
                     Suggest the user to use /act to switch to execution mode.",
                    env_pairs,
                ),
                ToolMode::Normal => bend_engine::tools::base_tools(env_pairs),
            };

            // Append MemoryTool (same pattern as SkillTool — constructed by app layer)
            if let Some(memory_tool) = super::prompt::memory::load_memory_tool(&self.cwd) {
                let memory_tool = match *self.tool_mode.read() {
                    ToolMode::Planning => memory_tool.disallow_writes(
                        "This tool is not allowed in planning mode. \
                         Suggest the user to use /act to switch to execution mode.",
                    ),
                    ToolMode::Normal => memory_tool,
                };
                t.push(Box::new(memory_tool));
            }

            t
        };

        let options = EngineOptions {
            provider: llm.provider,
            model: llm.model,
            api_key: llm.api_key,
            base_url: llm.base_url,
            system_prompt: self.build_system_prompt(),
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

    /// Start a side conversation — an independent, non-persisted agent loop.
    ///
    /// Uses the current LLM configuration with readonly tools and `MemoryStorage`
    /// so nothing is written to disk. The returned `SideAgent` reuses the
    /// standard `submit()` → `run_loop()` → `TurnStream` pipeline; multi-turn
    /// context is maintained in-memory by `Session`.
    pub fn start_side_conversation(self: &Arc<Self>, request: SideRequest) -> Result<SideAgent> {
        let llm = self.llm.read().clone();
        let mem_storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());

        let side_agent = Arc::new(Self {
            llm: RwLock::new(llm),
            system_prompt: RwLock::new(request.system_prompt),
            limits: RwLock::new(self.limits.read().clone()),
            skills_dirs: RwLock::new(vec![]),
            tool_mode: RwLock::new(ToolMode::Normal),
            cwd: self.cwd.clone(),
            storage: RwLock::new(mem_storage),
            variables: RwLock::new(None),
            tools_override: Some(bend_engine::tools::readonly_tools),
        });

        Ok(SideAgent {
            agent: side_agent,
            session_id: None,
        })
    }
}

// ---------------------------------------------------------------------------
// SideRequest
// ---------------------------------------------------------------------------

/// Request to start a side conversation.
/// Separate type from `TurnRequest` — different intent, different lifecycle.
pub struct SideRequest {
    /// System prompt for the side conversation.
    pub system_prompt: String,
}

// ---------------------------------------------------------------------------
// SideAgent
// ---------------------------------------------------------------------------

/// Handle for a side conversation.
///
/// Wraps an ephemeral `AppAgent` backed by `MemoryStorage`. Multi-turn context
/// is maintained in-memory by `Session`. Drop to discard — nothing is persisted.
///
/// The `session_id` on the returned `TurnStream` is a temporary identifier
/// (not resumable).
pub struct SideAgent {
    agent: Arc<AppAgent>,
    session_id: Option<String>,
}

impl SideAgent {
    /// Send a message. The first call creates a new in-memory session;
    /// subsequent calls reuse the same session for multi-turn context.
    pub async fn send(&mut self, prompt: &str) -> Result<TurnStream> {
        let request = TurnRequest::text(prompt).session_id(self.session_id.clone());
        let stream = self.agent.submit(request).await?;
        if self.session_id.is_none() {
            self.session_id = Some(stream.session_id.clone());
        }
        Ok(stream)
    }
}
