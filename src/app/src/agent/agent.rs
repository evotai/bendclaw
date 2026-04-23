use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use parking_lot::RwLock;

use super::run::convert;
use super::run::run::Run;
use super::run::runtime;
use super::session::Session;
use super::tools::build_tools;
use super::tools::ToolMode;
use super::variables::Variables;
use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::EvotError;
use crate::error::Result;
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
// QueryRequest
// ---------------------------------------------------------------------------

pub struct QueryRequest {
    pub input: Vec<evot_engine::Content>,
    pub session_id: Option<String>,
    pub mode: ToolMode,
    pub source: String,
}

impl QueryRequest {
    pub fn text(prompt: impl Into<String>) -> Self {
        Self {
            input: vec![evot_engine::Content::Text {
                text: prompt.into(),
            }],
            session_id: None,
            mode: ToolMode::Headless,
            source: String::new(),
        }
    }

    pub fn with_input(input: Vec<evot_engine::Content>) -> Self {
        Self {
            input,
            session_id: None,
            mode: ToolMode::Headless,
            source: String::new(),
        }
    }

    /// Extract plain text from input content (for transcript, titles, logs).
    pub fn input_text(&self) -> String {
        crate::agent::run::convert::extract_content_text(&self.input)
    }

    pub fn session_id(mut self, id: Option<String>) -> Self {
        self.session_id = id;
        self
    }

    pub fn mode(mut self, mode: ToolMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
}

// ---------------------------------------------------------------------------
// SubmitOutcome — result of a submit: either a Run or a handled command
// ---------------------------------------------------------------------------

pub enum SubmitOutcome {
    /// Normal agent run.
    Run(Run),
    /// A gateway command was handled; carry this text back to the caller.
    Command(String),
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

const PLANNING_MODE_PROMPT: &str = include_str!("prompt/plan.md");

struct ActiveRun {
    run_id: String,
    handle: evot_engine::RunHandle,
    done: Arc<AtomicBool>,
}

pub struct Agent {
    llm: RwLock<LlmConfig>,
    system_prompt: RwLock<String>,
    limits: RwLock<ExecutionLimits>,
    skills_dirs: RwLock<Vec<PathBuf>>,
    cwd: String,
    storage: RwLock<Arc<dyn Storage>>,
    variables: RwLock<Option<Arc<Variables>>>,
    sandbox: super::sandbox::SandboxPolicy,
    /// session_id → (run_id, handle, done_flag)
    active_runs: Arc<parking_lot::Mutex<HashMap<String, ActiveRun>>>,
}

impl Agent {
    pub fn new(config: &Config, cwd: impl Into<String>) -> Result<Arc<Self>> {
        let cwd = cwd.into();
        let storage = open_storage(&config.storage)?;
        let system_prompt = format!("You are a helpful assistant. Working directory: {cwd}");
        Ok(Arc::new(Self {
            llm: RwLock::new(config.active_llm()?),
            system_prompt: RwLock::new(system_prompt),
            limits: RwLock::new(ExecutionLimits::default()),
            skills_dirs: RwLock::new(Vec::new()),
            cwd,
            storage: RwLock::new(storage),
            variables: RwLock::new(None),
            sandbox: super::sandbox::SandboxPolicy::from_config(&config.sandbox),
            active_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
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

    pub fn set_llm(&self, llm: LlmConfig) {
        *self.llm.write() = llm;
    }

    /// Set the active model by spec (e.g. "deepseek-chat" or "openrouter:google/gemini-2.5-pro").
    /// Resolves provider+model from config. Falls back to just updating the model name.
    pub fn set_model_by_spec(&self, config: &Config, spec: &str) {
        if let Ok((provider_name, model_override)) = config.resolve_model_spec(spec) {
            if let Some(profile) = config.providers.get(&provider_name) {
                let llm = LlmConfig {
                    provider: provider_name,
                    protocol: profile.protocol.clone(),
                    api_key: profile.api_key.clone(),
                    base_url: profile.base_url.clone(),
                    model: model_override.unwrap_or_else(|| profile.model.clone()),
                    thinking_level: config.llm.thinking_level,
                };
                self.set_llm(llm);
                return;
            }
        }
        self.set_model(spec.to_string());
    }

    /// Switch provider by spec. Unlike `set_model_by_spec`, this fails if the spec
    /// cannot be resolved to a known provider.
    pub fn set_provider_by_spec(&self, config: &Config, spec: &str) -> Result<()> {
        let (provider_name, model_override) = config.resolve_model_spec(spec)?;
        let profile = config
            .providers
            .get(&provider_name)
            .ok_or_else(|| EvotError::Conf(format!("provider '{}' not found", provider_name)))?;
        let llm = LlmConfig {
            provider: provider_name,
            protocol: profile.protocol.clone(),
            api_key: profile.api_key.clone(),
            base_url: profile.base_url.clone(),
            model: model_override.unwrap_or_else(|| profile.model.clone()),
            thinking_level: config.llm.thinking_level,
        };
        self.set_llm(llm);
        Ok(())
    }

    pub fn variables(&self) -> Option<Arc<Variables>> {
        self.variables.read().clone()
    }

    pub fn storage(&self) -> Arc<dyn Storage> {
        self.storage.read().clone()
    }

    // -- run control ---------------------------------------------------------

    /// Send a steering message to the active run for a session.
    pub fn steer(&self, session_id: &str, input: Vec<evot_engine::Content>) {
        if let Some(ar) = self.active_runs.lock().get(session_id) {
            if !ar.done.load(Ordering::Relaxed) {
                ar.handle
                    .steer(evot_engine::AgentMessage::Llm(evot_engine::Message::User {
                        content: input,
                        timestamp: evot_engine::now_ms(),
                    }));
            }
        }
    }

    /// Send a follow-up message to the active run for a session.
    pub fn follow_up(&self, session_id: &str, text: impl Into<String>) {
        if let Some(ar) = self.active_runs.lock().get(session_id) {
            if !ar.done.load(Ordering::Relaxed) {
                ar.handle
                    .follow_up(evot_engine::AgentMessage::Llm(evot_engine::Message::user(
                        text,
                    )));
            }
        }
    }

    /// Abort the active run for a session.
    pub fn abort_run(&self, session_id: &str) {
        if let Some(ar) = self.active_runs.lock().get(session_id) {
            ar.handle.abort();
        }
    }

    /// Check if a session has an active (non-finished) run.
    /// Automatically cleans up finished runs.
    pub fn has_active_run(&self, session_id: &str) -> bool {
        let mut map = self.active_runs.lock();
        if let Some(ar) = map.get(session_id) {
            if ar.done.load(Ordering::Relaxed) {
                map.remove(session_id);
                return false;
            }
            true
        } else {
            false
        }
    }

    // -- query ---------------------------------------------------------------

    pub async fn submit(&self, request: QueryRequest) -> Result<SubmitOutcome> {
        let session = self
            .resolve_session(request.session_id.as_deref(), &request.source)
            .await?;
        self.submit_to_session(request, session).await
    }

    /// Channel path: session is already resolved by the caller (RunManager).
    /// Intercepts gateway commands before starting a run.
    pub async fn submit_to_session(
        &self,
        request: QueryRequest,
        session: Arc<Session>,
    ) -> Result<SubmitOutcome> {
        // Intercept gateway commands (/clear, /goto, ...)
        if let Some(msg) = self.maybe_handle_command(&request, &session).await? {
            return Ok(SubmitOutcome::Command(msg));
        }

        let run = self.start_run(request, session).await?;
        Ok(SubmitOutcome::Run(run))
    }

    // -- command handling (private) -------------------------------------------

    async fn maybe_handle_command(
        &self,
        request: &QueryRequest,
        session: &Arc<Session>,
    ) -> Result<Option<String>> {
        use crate::gateway::command::parse_command;
        use crate::gateway::command::Command;

        let cmd = match parse_command(&request.input_text()) {
            Some(cmd) => cmd,
            None => return Ok(None),
        };

        match cmd {
            Command::UsageError(msg) => Ok(Some(msg)),
            Command::Clear => {
                let session_id = session.session_id().await;
                self.abort_run(&session_id);
                session.write_clear_marker().await?;
                session.save().await?;
                Ok(Some("Session cleared.".into()))
            }
            Command::Goto(seq) => {
                if !session.is_valid_context_seq(seq).await? {
                    let max = session.max_seq().await;
                    return Ok(Some(format!(
                        "Invalid message number. Valid range: 1-{max}."
                    )));
                }
                let session_id = session.session_id().await;
                self.abort_run(&session_id);
                session.write_goto_marker(seq).await?;
                session.save().await?;
                // Show context window around the goto point
                let entries = session.recent_context_entries(5).await?;
                let mut lines = vec![format!("Moved to message #{seq}.")];
                for (s, item) in &entries {
                    let is_target = *s == seq;
                    let marker = if is_target { " ←" } else { "" };
                    lines.push(format!("  {}{}", format_history_entry(*s, item), marker));
                }
                // If target wasn't in the window (it's now in snapshot with seq=0),
                // show it explicitly
                if !entries.iter().any(|(s, _)| *s == seq) {
                    if let Some(item) = session.get_item_at(seq).await? {
                        lines.push(format!("  target: {} ←", format_history_entry(seq, &item)));
                    }
                }
                Ok(Some(lines.join("\n")))
            }
            Command::History(limit) => {
                let entries = session.recent_context_entries(limit).await?;
                if entries.is_empty() {
                    return Ok(Some("No messages in session.".into()));
                }
                let mut lines = Vec::new();
                for (seq, item) in &entries {
                    lines.push(format!("  {}", format_history_entry(*seq, item)));
                }
                Ok(Some(lines.join("\n")))
            }
        }
    }

    // -- run execution (private) ----------------------------------------------

    async fn start_run(&self, request: QueryRequest, session: Arc<Session>) -> Result<Run> {
        let session_id = session.meta().await.session_id.clone();
        let run_id = crate::types::new_id();

        // Session-level safety net: abort any existing active run for this session.
        // This ensures no two runs overlap on the same session, regardless of caller
        // (RunManager, HTTP, NAPI). Long-term this could be consolidated into a
        // single coordination layer if all entry points go through RunManager.
        if let Some(ar) = self.active_runs.lock().remove(&session_id) {
            ar.handle.abort();
        }

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

        // Shared done flag — set by on_complete, checked at registration
        let done = Arc::new(AtomicBool::new(false));

        // Build cleanup callback — mark done, remove only if still this run
        let active_runs = self.active_runs.clone();
        let sid = session_id.clone();
        let rid = run_id.clone();
        let done_flag = done.clone();
        let on_complete: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            done_flag.store(true, Ordering::Release);
            let mut map = active_runs.lock();
            if let Some(ar) = map.get(&sid) {
                if ar.run_id == rid {
                    map.remove(&sid);
                }
            }
        });

        let run = runtime::execute_turn(turn, Some(on_complete)).await?;

        // Register active run — skip if on_complete already fired
        if !done.load(Ordering::Acquire) {
            self.active_runs.lock().insert(session_id, ActiveRun {
                run_id,
                handle: run.handle(),
                done,
            });
        }

        Ok(run)
    }

    // -- fork ----------------------------------------------------------------

    /// Fork an independent, non-persisted agent for side conversations.
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
            active_runs: _,
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
            active_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
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

    pub async fn list_sessions_with_text(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::search::SessionWithText>> {
        let storage = self.storage.read().clone();
        storage.list_sessions_with_text(limit).await
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

    async fn resolve_session(
        &self,
        session_id: Option<&str>,
        source: &str,
    ) -> Result<Arc<Session>> {
        let model = self.llm.read().model.clone();
        let storage = self.storage.read().clone();
        match session_id {
            Some(id) => match Session::open(id, storage.clone()).await? {
                Some(session) => {
                    session.set_model(model).await;
                    Ok(session)
                }
                None => {
                    Session::new_with_source(
                        id.to_string(),
                        self.cwd.clone(),
                        model,
                        source,
                        storage,
                    )
                    .await
                }
            },
            None => {
                let id = crate::types::new_id();
                Session::new_with_source(id, self.cwd.clone(), model, source, storage).await
            }
        }
    }

    async fn build_turn(
        &self,
        request: &QueryRequest,
        session: Arc<Session>,
        session_id: &str,
        run_id: &str,
    ) -> Result<runtime::TurnInput> {
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
        let prior_messages = convert::into_agent_messages(&prior_transcripts);
        let prior_messages = evot_engine::sanitize_tool_pairs(prior_messages);

        Ok(runtime::TurnInput {
            options: runtime::EngineOptions {
                protocol: llm.protocol,
                model: llm.model,
                api_key: llm.api_key,
                base_url: Some(llm.base_url),
                system_prompt,
                limits: self.limits.read().clone(),
                skills_dirs: skill_dirs,
                tools,
                thinking_level: llm.thinking_level,
                cwd: cwd_path.to_path_buf(),
                path_guard: sandbox_rt.path_guard,
            },
            history: prior_messages,
            input: request.input.clone(),
            session,
            run_id: run_id.to_string(),
            session_id: session_id.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_history_entry(seq: u64, item: &crate::types::TranscriptItem) -> String {
    let role = match item {
        crate::types::TranscriptItem::User { .. } => "user",
        crate::types::TranscriptItem::Assistant { .. } => "assistant",
        _ => {
            debug_assert!(false, "history entry must be user or assistant");
            "unknown"
        }
    };
    let preview = crate::types::entry_preview(item);
    if seq == 0 {
        format!("  …   {:<10} {}", role, preview)
    } else {
        format!("#{:<4} {:<10} {}", seq, role, preview)
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
    pub async fn query(&mut self, prompt: &str) -> Result<Run> {
        let request = QueryRequest::text(prompt)
            .session_id(self.session_id.clone())
            .mode(ToolMode::Readonly);
        let outcome = self.agent.submit(request).await?;
        match outcome {
            SubmitOutcome::Run(run) => {
                if self.session_id.is_none() {
                    self.session_id = Some(run.session_id.clone());
                }
                Ok(run)
            }
            SubmitOutcome::Command(_) => Err(crate::error::EvotError::Run(
                "commands not supported in forked agent".into(),
            )),
        }
    }
}
