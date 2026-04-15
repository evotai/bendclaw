use std::path::PathBuf;
use std::sync::Arc;

use evot_engine::tools::*;
use parking_lot::RwLock;
use tokio::sync::mpsc;

use super::event::RunEvent;
use super::runtime::EngineHandle;
use super::variables::Variables;
use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::EvotError;
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
// ToolMode — determines which tools are registered for a query
// ---------------------------------------------------------------------------

pub enum ToolMode {
    /// REPL interactive: full tools + ask_user
    Interactive { ask_fn: AskUserFn },
    /// Oneshot / API / headless: full tools, no ask_user
    Headless,
    /// Plan mode: write tools degraded, optional ask_user
    Planning { ask_fn: Option<AskUserFn> },
    /// Forked conversation: read-only
    Readonly,
}

impl ToolMode {
    pub fn is_planning(&self) -> bool {
        matches!(self, Self::Planning { .. })
    }

    pub fn is_readonly(&self) -> bool {
        matches!(self, Self::Readonly)
    }
}

fn build_bash_tool(
    envs: Vec<(String, String)>,
    sandbox_dirs: Option<Vec<PathBuf>>,
) -> Box<dyn evot_engine::AgentTool> {
    let mut bash = BashTool::default().with_envs(envs);
    if let Some(dirs) = sandbox_dirs {
        bash = bash.with_sandbox_dirs(dirs);
    }
    Box::new(bash)
}

fn build_tools(
    mode: &ToolMode,
    envs: Vec<(String, String)>,
    allow_bash: bool,
    sandbox_dirs: Option<Vec<PathBuf>>,
) -> Vec<Box<dyn evot_engine::AgentTool>> {
    match mode {
        ToolMode::Interactive { ask_fn } => {
            let mut t: Vec<Box<dyn evot_engine::AgentTool>> = Vec::new();
            if allow_bash {
                t.push(build_bash_tool(envs, sandbox_dirs));
            }
            t.push(Box::new(ReadFileTool::default()));
            t.push(Box::new(WriteFileTool::new()));
            t.push(Box::new(EditFileTool::new()));
            t.push(Box::new(ListFilesTool::default()));
            t.push(Box::new(SearchTool::default()));
            t.push(Box::new(WebFetchTool::new()));
            t.push(Box::new(AskUserTool::new(ask_fn.clone())));
            t
        }
        ToolMode::Headless => {
            let mut t: Vec<Box<dyn evot_engine::AgentTool>> = Vec::new();
            if allow_bash {
                t.push(build_bash_tool(envs, sandbox_dirs));
            }
            t.push(Box::new(ReadFileTool::default()));
            t.push(Box::new(WriteFileTool::new()));
            t.push(Box::new(EditFileTool::new()));
            t.push(Box::new(ListFilesTool::default()));
            t.push(Box::new(SearchTool::default()));
            t.push(Box::new(WebFetchTool::new()));
            t
        }
        ToolMode::Planning { ask_fn } => {
            let msg = "Not allowed in planning mode. Use /act to switch.";
            let mut t: Vec<Box<dyn evot_engine::AgentTool>> = Vec::new();
            if allow_bash {
                t.push(build_bash_tool(envs, sandbox_dirs));
            }
            t.push(Box::new(ReadFileTool::default()));
            t.push(Box::new(WriteFileTool::new().disallow(msg)));
            t.push(Box::new(EditFileTool::new().disallow(msg)));
            t.push(Box::new(ListFilesTool::default()));
            t.push(Box::new(SearchTool::default()));
            t.push(Box::new(WebFetchTool::new()));
            if let Some(f) = ask_fn {
                t.push(Box::new(AskUserTool::new(f.clone())));
            }
            t
        }
        ToolMode::Readonly => vec![
            Box::new(ReadFileTool::default()),
            Box::new(ListFilesTool::default()),
            Box::new(SearchTool::default()),
        ],
    }
}

// ---------------------------------------------------------------------------
// QueryRequest
// ---------------------------------------------------------------------------

pub struct QueryRequest {
    pub prompt: String,
    pub session_id: Option<String>,
    pub mode: ToolMode,
}

impl QueryRequest {
    /// Headless query — no user interaction (default for oneshot / API).
    pub fn text(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            session_id: None,
            mode: ToolMode::Headless,
        }
    }

    pub fn session_id(mut self, id: Option<String>) -> Self {
        self.session_id = id;
        self
    }

    pub fn mode(mut self, mode: ToolMode) -> Self {
        self.mode = mode;
        self
    }
}

// ---------------------------------------------------------------------------
// QueryStream
// ---------------------------------------------------------------------------

pub struct QueryStream {
    pub(super) rx: mpsc::UnboundedReceiver<RunEvent>,
    pub session_id: String,
    pub run_id: String,
    pub(super) engine_handle: EngineHandle,
}

impl QueryStream {
    pub async fn next(&mut self) -> Option<RunEvent> {
        self.rx.recv().await
    }

    pub fn abort(&self) {
        self.engine_handle.abort();
    }

    /// Test-only constructor: create a QueryStream from a raw receiver.
    #[doc(hidden)]
    pub fn from_receiver(
        rx: mpsc::UnboundedReceiver<RunEvent>,
        session_id: String,
        run_id: String,
    ) -> Self {
        Self {
            rx,
            session_id,
            run_id,
            engine_handle: EngineHandle::noop(),
        }
    }
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

const PLANNING_MODE_PROMPT: &str = include_str!("prompt/plan.md");

pub struct Agent {
    llm: RwLock<LlmConfig>,
    system_prompt: RwLock<String>,
    limits: RwLock<ExecutionLimits>,
    skills_dirs: RwLock<Vec<PathBuf>>,
    cwd: String,
    storage: RwLock<Arc<dyn Storage>>,
    variables: RwLock<Option<Arc<Variables>>>,
    sandbox: super::sandbox::SandboxPolicy,
}

impl Agent {
    pub fn new(config: &Config, cwd: impl Into<String>) -> Result<Arc<Self>> {
        let cwd = cwd.into();
        let storage = open_storage(&config.storage)?;
        let system_prompt = format!("You are a helpful assistant. Working directory: {cwd}");
        Ok(Arc::new(Self {
            llm: RwLock::new(config.active_llm()),
            system_prompt: RwLock::new(system_prompt),
            limits: RwLock::new(ExecutionLimits::default()),
            skills_dirs: RwLock::new(Vec::new()),
            cwd,
            storage: RwLock::new(storage),
            variables: RwLock::new(None),
            sandbox: super::sandbox::SandboxPolicy::from_config(&config.sandbox),
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

    fn with_claude_skills_dirs(self: &Arc<Self>) -> Arc<Self> {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let claude_dir = PathBuf::from(home).join(".claude").join("skills");
            if claude_dir.is_dir() {
                let mut dirs = self.skills_dirs.write();
                if !dirs.contains(&claude_dir) {
                    dirs.push(claude_dir);
                }
            }
        }
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

    pub fn variables(&self) -> Option<Arc<Variables>> {
        self.variables.read().clone()
    }

    pub fn storage(&self) -> Arc<dyn Storage> {
        self.storage.read().clone()
    }

    // -- query ---------------------------------------------------------------

    pub async fn query(&self, request: QueryRequest) -> Result<QueryStream> {
        let session = self.resolve_session(request.session_id.as_deref()).await?;
        let session_id = session.meta().await.session_id.clone();
        let run_id = crate::types::new_id();

        tracing::info!(
            stage = "run",
            status = "started",
            run_id = %run_id,
            session_id = %session_id,
            provider = ?self.llm.read().provider,
            model = %self.llm.read().model,
        );

        let turn = self
            .build_turn(&request, session, &session_id, &run_id)
            .await?;
        super::runtime::execute_turn(turn).await
    }

    // -- fork ----------------------------------------------------------------

    /// Fork an independent, non-persisted agent for side conversations.
    ///
    /// Uses the current LLM configuration with readonly tools and in-memory
    /// storage. The returned `ForkedAgent` maintains multi-turn context
    /// in-memory via `Session`. Drop to discard — nothing is persisted.
    pub fn fork(self: &Arc<Self>, request: ForkRequest) -> Result<ForkedAgent> {
        let Self {
            llm,
            system_prompt: _,
            limits,
            skills_dirs: _,
            cwd,
            storage: _,
            variables: _,
            sandbox,
        } = self.as_ref();

        let forked = Arc::new(Self {
            llm: RwLock::new(llm.read().clone()),
            system_prompt: RwLock::new(request.system_prompt),
            limits: RwLock::new(limits.read().clone()),
            skills_dirs: RwLock::new(vec![]),
            cwd: cwd.clone(),
            storage: RwLock::new(Arc::new(MemoryStorage::new())),
            variables: RwLock::new(None),
            sandbox: super::sandbox::SandboxPolicy {
                enabled: sandbox.enabled,
                extra_dirs: sandbox.extra_dirs.clone(),
            },
        });
        Ok(ForkedAgent {
            agent: forked,
            session_id: None,
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

    // -- private -------------------------------------------------------------

    fn build_system_prompt(&self, mode: &ToolMode) -> String {
        let base = self.system_prompt.read().clone();
        let mut prompt = match mode {
            ToolMode::Planning { .. } => format!("{base}\n\n{PLANNING_MODE_PROMPT}"),
            _ => base,
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

        if self.sandbox.enabled {
            prompt.push_str(
                "\n\n# Sandbox Mode\n\
                 You are running in a sandboxed environment with OS-level filesystem restrictions.\n\
                 - File access is restricted to the project workspace and explicitly allowed directories.\n\
                 - The user's home directory ($HOME) is NOT accessible except for allowed paths.\n\
                 - Do NOT attempt to install packages (pip install, brew install, curl | sh, etc.) — \
                 they will fail with \"Operation not permitted\".\n\
                 - Do NOT retry commands that fail with permission errors — the restriction is \
                 enforced by the kernel and cannot be bypassed.\n\
                 - Use only tools and binaries already available on PATH.",
            );
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
                None => Err(EvotError::Session(format!("session not found: {id}"))),
            },
            None => {
                let id = crate::types::new_id();
                Session::new(id, self.cwd.clone(), model, storage).await
            }
        }
    }

    async fn build_turn(
        &self,
        request: &QueryRequest,
        session: Arc<Session>,
        session_id: &str,
        run_id: &str,
    ) -> Result<super::runtime::TurnInput> {
        let llm = self.llm.read().clone();
        let system_prompt = self.build_system_prompt(&request.mode);
        let envs = self
            .variables()
            .map(|v| v.all_env_pairs())
            .unwrap_or_default();
        // Build path guard from sandbox policy
        let cwd_path = std::path::Path::new(&self.cwd);
        let memory_dirs = super::prompt::memory::resolve_memory_dirs(&self.cwd);
        let skill_dirs = self.skills_dirs.read().clone();
        let sandbox_rt = self
            .sandbox
            .build_runtime(cwd_path, &memory_dirs, &skill_dirs)?;

        let mut tools = build_tools(
            &request.mode,
            envs,
            sandbox_rt.allow_bash,
            sandbox_rt.bash_sandbox_dirs,
        );

        if !request.mode.is_readonly() {
            if let Some(mt) = super::prompt::memory::load_memory_tool(&self.cwd) {
                if request.mode.is_planning() {
                    tools.push(Box::new(mt.disallow_writes(
                        "Not allowed in planning mode. Use /act to switch.",
                    )));
                } else {
                    tools.push(Box::new(mt));
                }
            }
        }

        let prior_transcripts = session.transcript().await;
        let prior_messages = super::convert::into_agent_messages(&prior_transcripts);
        let prior_messages = evot_engine::sanitize_tool_pairs(prior_messages);

        Ok(super::runtime::TurnInput {
            options: super::runtime::EngineOptions {
                provider: llm.provider,
                model: llm.model,
                api_key: llm.api_key,
                base_url: llm.base_url,
                system_prompt,
                limits: self.limits.read().clone(),
                skills_dirs: skill_dirs,
                tools,
                thinking_level: llm.thinking_level,
                cwd: cwd_path.to_path_buf(),
                path_guard: sandbox_rt.path_guard,
            },
            prior_messages,
            prompt: request.prompt.clone(),
            session,
            run_id: run_id.to_string(),
            session_id: session_id.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// ForkRequest / ForkedAgent
// ---------------------------------------------------------------------------

pub struct ForkRequest {
    pub system_prompt: String,
}

/// Handle for a forked conversation.
///
/// Wraps an ephemeral `Agent` backed by `MemoryStorage`. Multi-turn context
/// is maintained in-memory by `Session`. Drop to discard — nothing is persisted.
pub struct ForkedAgent {
    agent: Arc<Agent>,
    session_id: Option<String>,
}

impl ForkedAgent {
    pub async fn query(&mut self, prompt: &str) -> Result<QueryStream> {
        let request = QueryRequest::text(prompt)
            .session_id(self.session_id.clone())
            .mode(ToolMode::Readonly);
        let stream = self.agent.query(request).await?;
        if self.session_id.is_none() {
            self.session_id = Some(stream.session_id.clone());
        }
        Ok(stream)
    }
}
