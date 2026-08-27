use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use super::run::control::RunControl;
use super::run::run::Run;
use super::run::runtime;
use super::run::runtime::TurnFactory;
use super::session::Session;
use super::tools::build_tools;
use super::tools::HostTools;
use super::tools::ToolMode;
use super::variables::Variables;
use crate::agent::prompt::dynamic_sections;
use crate::agent::prompt::format_skills_for_prompt;
use crate::agent::prompt::DynamicContext;
use crate::agent::prompt::PromptMode;
use crate::agent::prompt::Section;
use crate::agent::prompt::SkillSpec;
use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::conf::Protocol;
use crate::error::EvotError;
use crate::error::Result;
use crate::storage::open_storage;
use crate::storage::MemoryStorage;
use crate::storage::Storage;
use crate::types::ListSessions;
use crate::types::ListTranscriptEntries;
use crate::types::PromptDump;
use crate::types::SectionDump;
use crate::types::SessionMeta;
use crate::types::SystemPromptDump;
use crate::types::TokenTotals;
use crate::types::ToolDump;
use crate::types::TranscriptEntry;
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
    /// Optional per-run model snapshot. `submit` fills this from the agent when
    /// omitted, so every run uses one stable selection across all turns.
    pub llm: Option<LlmConfig>,
    /// Host-owned tools (ask_user, …) to attach to this run. `None` when the
    /// caller has no host bridge (e.g. gateway/headless callers).
    pub host_tools: Option<HostTools>,
    /// Workspace for a *new* session. Existing sessions always keep the cwd
    /// persisted on `SessionMeta`; this field is ignored on resume.
    pub cwd: Option<String>,
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
            llm: None,
            host_tools: None,
            cwd: None,
        }
    }

    pub fn with_input(input: Vec<evot_engine::Content>) -> Self {
        Self {
            input,
            session_id: None,
            mode: ToolMode::Headless,
            source: String::new(),
            llm: None,
            host_tools: None,
            cwd: None,
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

    /// Pin a resolved model selection to this run. Useful for callers such as
    /// Chat that expose a per-message model picker.
    pub fn llm(mut self, llm: LlmConfig) -> Self {
        self.llm = Some(llm);
        self
    }

    /// Attach host-owned tools (the host bridge plus its registered specs).
    pub fn host_tools(mut self, host_tools: Option<HostTools>) -> Self {
        self.host_tools = host_tools;
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// Bind a workspace directory for a newly created session. Resume always
    /// keeps the persisted session cwd, so this is a no-op once a session id
    /// already exists.
    pub fn cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

/// Expand `/clip all` into a prompt with the memory workflow loaded.
/// Non-command input passes through unchanged.
fn expand_prompt_command(
    mut request: QueryRequest,
    skills_dirs: &[PathBuf],
) -> Result<QueryRequest> {
    use crate::command::clip_session_prompt;
    use crate::command::parse_command;
    use crate::command::Command;

    if !matches!(
        parse_command(&request.input_text()),
        Some(Command::ClipSession)
    ) {
        return Ok(request);
    }
    let memory = crate::agent::prompt::skill::load_skill(skills_dirs, "memory")
        .map_err(|error| EvotError::Agent(format!("cannot load memory skill: {error}")))?;
    let instructions = crate::agent::prompt::skill::load_skill_instructions(&memory)
        .map_err(|error| EvotError::Agent(format!("cannot read memory skill: {error}")))?;
    let text = clip_session_prompt(&instructions);
    request.input = vec![evot_engine::Content::Text { text }];
    Ok(request)
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

struct ActiveRun {
    run_id: String,
    handle: RunControl,
    completed: tokio_util::sync::CancellationToken,
}

enum AbortRunOutcome {
    Stopped,
    Cancelled,
    TimedOut,
}

const RUN_ABORT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const COMPACTION_SUMMARY_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Agent {
    llm: RwLock<LlmConfig>,
    system_prompt: RwLock<String>,
    /// Per-section breakdown matching `system_prompt`. Used by `/_dump`.
    /// Empty when `with_system_prompt` was called with a raw string and no
    /// sections; the dump path then treats the whole prompt as a single
    /// "system_prompt" section.
    system_prompt_sections: RwLock<Vec<Section>>,
    limits: RwLock<ExecutionLimits>,
    skills_dirs: RwLock<Vec<PathBuf>>,
    skill_names: RwLock<Option<Vec<String>>>,
    cwd: String,
    /// Root dir for spill files. Only set when storage backend is Fs.
    spill_root: Option<PathBuf>,
    storage: RwLock<Arc<dyn Storage>>,
    variables: RwLock<Option<Arc<Variables>>>,
    sandbox: super::sandbox::SandboxPolicy,
    provider_override: RwLock<Option<Arc<dyn evot_engine::provider::StreamProvider>>>,
    /// session_id → (run_id, handle, done_flag)
    active_runs: Arc<parking_lot::Mutex<HashMap<String, ActiveRun>>>,
    /// Premium landing model for new sessions; None without a special tier.
    new_session_llm: Option<LlmConfig>,
}

impl Agent {
    pub fn new(config: &Config, cwd: impl Into<String>) -> Result<Arc<Self>> {
        let cwd = cwd.into();
        let storage = open_storage(&config.storage)?;
        Ok(Arc::new(Self::new_inner(config, cwd, storage)?))
    }

    fn new_inner(config: &Config, cwd: String, storage: Arc<dyn Storage>) -> Result<Self> {
        let system_prompt = format!("You are a helpful assistant. Working directory: {cwd}");
        // Premium tier wins for fresh sessions; BYOK/Free-only stay None.
        let new_session_llm = config
            .preferred_new_session_llm()
            .and_then(|(provider, model)| {
                let llm = config.build_llm(&provider, Some(model));
                if let Err(error) = &llm {
                    tracing::warn!(%error, "premium landing model unusable; keeping live selection");
                }
                llm.ok()
            });
        Ok(Self {
            llm: RwLock::new(
                config
                    .active_llm()
                    .unwrap_or_else(|_| LlmConfig::unconfigured()),
            ),
            system_prompt: RwLock::new(system_prompt),
            system_prompt_sections: RwLock::new(Vec::new()),
            limits: RwLock::new(ExecutionLimits::default()),
            skills_dirs: RwLock::new(Vec::new()),
            skill_names: RwLock::new(None),
            cwd,
            spill_root: match config.storage.backend {
                crate::conf::StorageBackend::Fs => Some(config.storage.fs.root_dir.clone()),
                _ => None,
            },
            storage: RwLock::new(storage),
            variables: RwLock::new(None),
            sandbox: super::sandbox::SandboxPolicy::from_config(&config.sandbox),
            provider_override: RwLock::new(None),
            active_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            new_session_llm,
        })
    }

    pub fn new_with_provider_for_test(
        config: &Config,
        cwd: impl Into<String>,
        storage: Arc<dyn Storage>,
        provider: impl evot_engine::provider::StreamProvider + 'static,
    ) -> Result<Arc<Self>> {
        let agent = Arc::new(Self::new_inner(config, cwd.into(), storage)?);
        *agent.provider_override.write() = Some(Arc::new(provider));
        Ok(agent)
    }

    // -- configuration (fluent setters) --------------------------------------

    pub fn with_system_prompt(self: &Arc<Self>, prompt: impl Into<String>) -> Arc<Self> {
        let prompt = prompt.into();
        let mut current_prompt = self.system_prompt.write();
        let mut sections = self.system_prompt_sections.write();
        *current_prompt = prompt;
        sections.clear();
        Arc::clone(self)
    }

    /// Set the system prompt along with its per-section breakdown. The joined
    /// `text` must equal `sections` joined by `"\n\n"` — same invariant as
    /// `SystemPrompt::build_with_sections`.
    pub fn with_system_prompt_sections(
        self: &Arc<Self>,
        text: String,
        sections: Vec<Section>,
    ) -> Arc<Self> {
        let mut current_prompt = self.system_prompt.write();
        let mut current_sections = self.system_prompt_sections.write();
        *current_prompt = text;
        *current_sections = sections;
        Arc::clone(self)
    }

    /// Insert extra instructions where pi places `appendSystemPrompt`: after
    /// guidelines and before project context and the working directory.
    pub fn append_system_prompt(self: &Arc<Self>, extra: &str) -> Arc<Self> {
        if extra.is_empty() {
            return Arc::clone(self);
        }

        let mut prompt = self.system_prompt.write();
        let mut sections = self.system_prompt_sections.write();
        if sections.is_empty() {
            if !prompt.is_empty() {
                prompt.push_str("\n\n");
            }
            prompt.push_str(extra);
            return Arc::clone(self);
        }

        let insert_at = match sections.iter().position(|section| {
            matches!(
                section.name,
                "project_context" | "environment" | "dynamic_boundary"
            )
        }) {
            Some(index) => index,
            None => sections.len(),
        };
        sections.insert(insert_at, Section {
            name: "append",
            text: extra.to_string(),
        });
        *prompt = sections
            .iter()
            .map(|section| section.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
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

    pub fn add_skills_dirs(self: &Arc<Self>, dirs: Vec<PathBuf>) -> Arc<Self> {
        {
            let mut current = self.skills_dirs.write();
            for dir in dirs {
                if !current.contains(&dir) {
                    current.push(dir);
                }
            }
        }
        self.with_claude_skills_dirs()
    }

    pub fn set_skill_names(&self, names: Vec<String>) -> Result<()> {
        crate::agent::prompt::skill::load_skills_by_name(&self.skills_dirs(), &names)
            .map_err(|error| EvotError::Agent(error.to_string()))?;
        *self.skill_names.write() = Some(names);
        Ok(())
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

    /// The fully-resolved, ordered list of skills directories the agent scans:
    /// managed builtins, global `~/.evotai/skills`, config dirs, then
    /// `~/.claude/skills`. This is the single source of truth the CLI display
    /// layer should read so `/skill list` and the banner never drift from what
    /// the agent actually loads.
    pub fn skills_dirs(&self) -> Vec<PathBuf> {
        self.skills_dirs.read().clone()
    }

    pub fn limits(&self) -> ExecutionLimits {
        self.limits.read().clone()
    }

    pub fn set_llm(&self, llm: LlmConfig) {
        *self.llm.write() = llm;
    }

    /// Set the active thinking level for the current provider.
    pub fn set_thinking_level(&self, level: evot_engine::ThinkingLevel) {
        self.llm.write().thinking_level = level;
    }

    /// Apply a named thinking level when supported by the active model.
    /// Kept as a public API for callers that explicitly manage live state;
    /// session resume intentionally reloads the current configured value instead.
    pub fn restore_thinking_level(&self, name: &str) {
        let Ok(level) = crate::conf::thinking_level_from_str(name) else {
            return;
        };
        if self.supported_thinking_levels().contains(&level) {
            self.set_thinking_level(level);
        }
    }

    /// Thinking levels the current model can cycle through, in ascending order
    /// of effort. Empty when the model does not honor a reasoning effort (e.g.
    /// an OpenAI-compatible provider without the reasoning-effort capability).
    pub fn supported_thinking_levels(&self) -> Vec<evot_engine::ThinkingLevel> {
        let llm = self.llm.read().clone();
        Self::supported_thinking_levels_for(&llm)
    }

    fn supported_thinking_levels_for(llm: &LlmConfig) -> Vec<evot_engine::ThinkingLevel> {
        let model = Self::model_config_for(llm);
        if model.reasoning() {
            model.supported_thinking_levels()
        } else {
            Vec::new()
        }
    }

    /// Return the catalog + route + override metadata resolved by configuration.
    fn model_config_for(llm: &LlmConfig) -> evot_engine::provider::ModelConfig {
        llm.model_config.clone()
    }

    /// Replace the LLM config while inheriting the session's current thinking
    /// level, clamped to what the new model supports. Models without selectable
    /// reasoning always use `Off`. Cloud models with a catalog default skip
    /// this and take that default instead — the server is choosing the effort.
    fn set_llm_preserving_thinking(&self, mut llm: LlmConfig) {
        let current = self.llm.read().thinking_level;
        llm.thinking_level = Self::model_config_for(&llm).effective_thinking_level(current);
        self.set_llm(llm);
    }

    fn set_llm_for_model_switch(&self, config: &Config, llm: LlmConfig) {
        if config.cloud_thinking_levels.contains_key(&llm.model) {
            self.set_llm(llm);
            return;
        }
        self.set_llm_preserving_thinking(llm);
    }

    /// The active model's resolved context window in tokens, after applying
    /// explicit overrides. Used to size and validate compaction so the retained
    /// context fits what the model actually accepts.
    pub fn resolved_context_window(&self) -> u32 {
        Self::model_config_for(&self.llm.read()).context_window()
    }

    /// Advance the thinking level to the next supported tier, wrapping around.
    /// Returns the new level, or `None` when the model has no selectable levels.
    pub fn cycle_thinking_level(&self) -> Option<evot_engine::ThinkingLevel> {
        let levels = self.supported_thinking_levels();
        if levels.is_empty() {
            return None;
        }
        let current = self.llm.read().thinking_level;
        let next_index = levels
            .iter()
            .position(|l| *l == current)
            .map(|i| (i + 1) % levels.len())
            .unwrap_or(0);
        let next = levels[next_index];
        self.set_thinking_level(next);
        Some(next)
    }

    /// Set the active model by spec (e.g. "deepseek-chat" or "openrouter:google/gemini-2.5-pro").
    ///
    /// Resolution and provider config errors are returned before mutating the
    /// active LLM. Explicit `provider:model` remains the escape hatch for model
    /// ids not listed in config, as long as the provider itself exists.
    pub fn set_model_by_spec(&self, config: &Config, spec: &str) -> Result<()> {
        let (provider_name, model_override) = config.resolve_model_spec(spec)?;
        let llm = config.build_llm(&provider_name, model_override)?;
        self.set_llm_for_model_switch(config, llm);
        Ok(())
    }

    /// Switch provider by spec while preserving the live thinking level.
    /// Used for interactive model changes.
    pub fn set_provider_by_spec(&self, config: &Config, spec: &str) -> Result<()> {
        let (provider_name, model_override) = config.resolve_model_spec(spec)?;
        let llm = config.build_llm(&provider_name, model_override)?;
        self.set_llm_for_model_switch(config, llm);
        Ok(())
    }

    /// Reload a session's provider/model from current config. If that saved
    /// selection no longer exists, reapply config to the current live selection
    /// so its thinking level still refreshes. Returns whether the saved
    /// selection was restored; neither path mutates until resolution succeeds.
    pub fn reload_provider_for_resume(&self, config: &Config, spec: &str) -> Result<bool> {
        match config.resolve_model_spec(spec) {
            Ok((provider_name, model_override)) => {
                let llm = config.build_llm(&provider_name, model_override)?;
                self.set_llm(llm);
                Ok(true)
            }
            Err(saved_error) => {
                let current_spec = {
                    let llm = self.llm.read();
                    format!("{}:{}", llm.provider, llm.model)
                };
                let (provider_name, model_override) = config
                    .resolve_model_spec(&current_spec)
                    .map_err(|_| saved_error)?;
                let llm = config.build_llm(&provider_name, model_override)?;
                self.set_llm(llm);
                Ok(false)
            }
        }
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
            if !ar.completed.is_cancelled() {
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
            if !ar.completed.is_cancelled() {
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
            if ar.completed.is_cancelled() {
                map.remove(session_id);
                return false;
            }
            true
        } else {
            false
        }
    }

    /// Abort the current run for a session and wait until its cleanup callback
    /// has completed. Returns whether a run was active when the request began.
    pub async fn abort_run_and_wait_for_completion(&self, session_id: &str) -> Result<bool> {
        let active = self.has_active_run(session_id);
        if !active {
            return Ok(false);
        }
        let cancel = tokio_util::sync::CancellationToken::new();
        match self.abort_run_and_wait(session_id, &cancel).await {
            AbortRunOutcome::Stopped => Ok(true),
            AbortRunOutcome::Cancelled => Err(EvotError::Run(
                "run abort was unexpectedly cancelled".to_string(),
            )),
            AbortRunOutcome::TimedOut => Err(EvotError::Run(format!(
                "active run did not stop within {} seconds",
                RUN_ABORT_WAIT_TIMEOUT.as_secs()
            ))),
        }
    }

    async fn abort_run_and_wait(
        &self,
        session_id: &str,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> AbortRunOutcome {
        let active = {
            let map = self.active_runs.lock();
            map.get(session_id).map(|active| {
                active.handle.abort();
                active.completed.clone()
            })
        };
        let Some(completed) = active else {
            return AbortRunOutcome::Stopped;
        };
        if completed.is_cancelled() {
            return AbortRunOutcome::Stopped;
        }

        tokio::select! {
            _ = cancel.cancelled() => AbortRunOutcome::Cancelled,
            _ = completed.cancelled() => AbortRunOutcome::Stopped,
            _ = tokio::time::sleep(RUN_ABORT_WAIT_TIMEOUT) => {
                tracing::warn!(
                    stage = "compact",
                    status = "run_abort_timeout",
                    session_id = %session_id,
                    timeout_ms = RUN_ABORT_WAIT_TIMEOUT.as_millis() as u64,
                );
                AbortRunOutcome::TimedOut
            }
        }
    }

    /// Manually compact an existing session with an abortable lifecycle.
    pub async fn compact(
        &self,
        session_id: &str,
        custom_instructions: Option<String>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<crate::compact::orchestrator::ManualCompactionOutcome> {
        self.compact_with_observer(session_id, custom_instructions, cancel, None)
            .await
    }

    pub async fn compact_with_observer(
        &self,
        session_id: &str,
        custom_instructions: Option<String>,
        cancel: tokio_util::sync::CancellationToken,
        observer: Option<crate::compact::orchestrator::ManualCompactionObserver>,
    ) -> Result<crate::compact::orchestrator::ManualCompactionOutcome> {
        match self.abort_run_and_wait(session_id, &cancel).await {
            AbortRunOutcome::Stopped => {}
            AbortRunOutcome::Cancelled => {
                return Ok(crate::compact::orchestrator::ManualCompactionOutcome::Cancelled)
            }
            AbortRunOutcome::TimedOut => {
                return Err(EvotError::Run(format!(
                    "active run did not stop within {} seconds; compaction was not started",
                    RUN_ABORT_WAIT_TIMEOUT.as_secs()
                )))
            }
        }
        let Some(session) = self.load_session(session_id).await? else {
            return Ok(crate::compact::orchestrator::ManualCompactionOutcome::NothingToCompact);
        };
        self.compact_resolved_session(&session, custom_instructions, cancel, observer)
            .await
    }

    // -- query ---------------------------------------------------------------

    pub async fn submit(self: &Arc<Self>, mut request: QueryRequest) -> Result<SubmitOutcome> {
        // Session-independent commands are handled before resolve_session,
        // which would otherwise persist an empty session when the caller has
        // no session yet (e.g. `/resume <query>` from a fresh CLI).
        if let Some(crate::command::Command::ResumeSearch { query }) =
            crate::command::parse_command(&request.input_text())
        {
            let msg = self.handle_resume_search(&query).await?;
            return Ok(SubmitOutcome::Command(msg));
        }

        // Freeze one selection for session metadata and every turn in this run.
        // Without this snapshot, concurrent callers changing the live model
        // could make a run start or auto-continue on a different provider.
        let llm = request.llm.clone().unwrap_or_else(|| self.llm());
        request.llm = Some(llm.clone());
        let session = self
            .resolve_session(
                request.session_id.as_deref(),
                &request.source,
                &llm,
                request.cwd.as_deref(),
            )
            .await?;
        self.submit_to_session(request, session).await
    }

    /// Channel path: session is already resolved by the caller (RunManager).
    /// Intercepts gateway commands before starting a run.
    pub async fn submit_to_session(
        self: &Arc<Self>,
        request: QueryRequest,
        session: Arc<Session>,
    ) -> Result<SubmitOutcome> {
        // Intercept gateway commands (/clear, /compact, ...)
        if let Some(outcome) = self.maybe_handle_command(&request, &session).await? {
            return Ok(outcome);
        }
        // `/clip all` loads the memory workflow and continues as a normal run.
        let skills_dirs = self.skills_dirs.read().clone();
        let request = expand_prompt_command(request, &skills_dirs)?;

        let run = self.start_run(request, session).await?;
        Ok(SubmitOutcome::Run(run))
    }

    // -- command handling (private) -------------------------------------------

    async fn maybe_handle_command(
        self: &Arc<Self>,
        request: &QueryRequest,
        session: &Arc<Session>,
    ) -> Result<Option<SubmitOutcome>> {
        use crate::command::parse_command;
        use crate::command::Command;

        let cmd = match parse_command(&request.input_text()) {
            Some(cmd) => cmd,
            None => return Ok(None),
        };

        match cmd {
            Command::UsageError(msg) => Ok(Some(SubmitOutcome::Command(msg))),
            Command::Clear => {
                let session_id = session.session_id().await;
                self.abort_run(&session_id);
                session.write_clear_marker().await?;
                session.save().await?;
                Ok(Some(SubmitOutcome::Command("Session cleared.".into())))
            }
            Command::Compact {
                custom_instructions,
            } => {
                let session_id = session.session_id().await;
                let cancel = tokio_util::sync::CancellationToken::new();
                let outcome = match self.abort_run_and_wait(&session_id, &cancel).await {
                    AbortRunOutcome::Stopped => {
                        self.compact_resolved_session(session, custom_instructions, cancel, None)
                            .await?
                    }
                    AbortRunOutcome::Cancelled => {
                        crate::compact::orchestrator::ManualCompactionOutcome::Cancelled
                    }
                    AbortRunOutcome::TimedOut => {
                        return Err(EvotError::Run(format!(
                            "active run did not stop within {} seconds; compaction was not started",
                            RUN_ABORT_WAIT_TIMEOUT.as_secs()
                        )))
                    }
                };
                let msg = format_manual_compaction_outcome(&outcome);
                Ok(Some(SubmitOutcome::Command(msg)))
            }
            Command::Dump { target } => {
                let msg = self
                    .handle_dump_command(request.mode, session, target.as_deref())
                    .await?;
                Ok(Some(SubmitOutcome::Command(msg)))
            }
            // Expanded into a normal prompt by `expand_prompt_command` after
            // this interception step; nothing to handle here.
            Command::ClipSession => Ok(None),
            // Semantic session search — one-shot LLM ranking, no agent run.
            Command::ResumeSearch { query } => {
                let msg = self.handle_resume_search(&query).await?;
                Ok(Some(SubmitOutcome::Command(msg)))
            }
        }
    }

    // -- run execution (private) ----------------------------------------------

    async fn compact_resolved_session(
        &self,
        session: &Arc<Session>,
        custom_instructions: Option<String>,
        cancel: tokio_util::sync::CancellationToken,
        observer: Option<crate::compact::orchestrator::ManualCompactionObserver>,
    ) -> Result<crate::compact::orchestrator::ManualCompactionOutcome> {
        if cancel.is_cancelled() {
            return Ok(crate::compact::orchestrator::ManualCompactionOutcome::Cancelled);
        }
        let context_window = self.resolved_context_window() as usize;
        let request = crate::compact::orchestrator::ManualCompactRequest {
            reason: crate::types::CompactReason::Manual,
            custom_instructions,
            summary_override: None,
            summarizer: Some(self.compact_summarizer()),
            settings: crate::compact::orchestrator::CompactSettings {
                context_window,
                ..Default::default()
            },
            observer,
        };
        let result = crate::compact::orchestrator::compact_session_with_status(
            session,
            request,
            cancel.clone(),
        )
        .await?;
        if result.status == crate::compact::orchestrator::CompactSessionStatus::Cancelled {
            return Ok(crate::compact::orchestrator::ManualCompactionOutcome::Cancelled);
        }
        session.save().await?;
        match result.item {
            Some(crate::types::TranscriptItem::Compact {
                summary,
                tokens_before,
                tokens_after,
                messages_before,
                messages_after,
                details,
                ..
            }) => Ok(
                crate::compact::orchestrator::ManualCompactionOutcome::Compacted {
                    summary,
                    tokens_before,
                    tokens_after,
                    messages_before,
                    messages_after,
                    context_window,
                    messages_evicted: messages_before
                        .saturating_sub(messages_after)
                        .saturating_add(1),
                    current_run_reclaimed: 0,
                    compaction_level: 3,
                    used_fallback: result.used_fallback,
                    method: details.method,
                    remote_blob_bytes: details.remote_blob_bytes,
                    fallback_reason: details.fallback_reason,
                },
            ),
            _ => Ok(crate::compact::orchestrator::ManualCompactionOutcome::NothingToCompact),
        }
    }

    fn llm_provider(&self, protocol: &Protocol) -> Arc<dyn evot_engine::provider::StreamProvider> {
        use evot_engine::provider::AnthropicProvider;
        use evot_engine::provider::OpenAiCompatProvider;
        use evot_engine::provider::OpenAiResponsesProvider;

        self.provider_override
            .read()
            .clone()
            .unwrap_or_else(|| match protocol {
                Protocol::Anthropic => Arc::new(AnthropicProvider),
                Protocol::OpenAiResponses => Arc::new(OpenAiResponsesProvider),
                Protocol::OpenAi => Arc::new(OpenAiCompatProvider),
            })
    }

    fn compact_summarizer(&self) -> crate::compact::orchestrator::CompactSummarizer {
        let llm = self.llm.read().clone();
        let provider = self.llm_provider(&llm.protocol);
        crate::compact::orchestrator::CompactSummarizer {
            provider,
            llm,
            reserve_tokens: evot_engine::DEFAULT_SUMMARY_RESERVE_TOKENS,
            timeout: COMPACTION_SUMMARY_TIMEOUT,
        }
    }

    async fn start_run(
        self: &Arc<Self>,
        request: QueryRequest,
        session: Arc<Session>,
    ) -> Result<Run> {
        let session_id = session.meta().await.session_id.clone();
        let run_id = crate::types::new_id();
        // `submit_to_session` is also public and may bypass `submit`, so keep a
        // fallback snapshot here for channel callers that did not pin one.
        let llm = request.llm.clone().unwrap_or_else(|| self.llm());
        session
            .set_model_selection(llm.provider.clone(), llm.model.clone())
            .await?;
        session
            .set_thinking_level(Self::persisted_thinking_level_for(&llm))
            .await;

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
            provider = ?llm.provider,
            model = %llm.model,
        );

        // Completion is a one-shot signal, not a polled flag. This avoids a
        // manual compaction waiting forever on stale run state.
        let completed = tokio_util::sync::CancellationToken::new();

        // Build cleanup callback — signal completion, remove only if still this run
        let active_runs = self.active_runs.clone();
        let sid = session_id.clone();
        let rid = run_id.clone();
        let completed_signal = completed.clone();
        let on_complete: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            completed_signal.cancel();
            let mut map = active_runs.lock();
            if let Some(ar) = map.get(&sid) {
                if ar.run_id == rid {
                    map.remove(&sid);
                }
            }
        });

        let factory: Arc<dyn TurnFactory> = Arc::new(AgentTurnFactory {
            agent: Arc::clone(self),
            session: Arc::clone(&session),
            mode: request.mode,
            session_id: session_id.clone(),
            llm,
            host_tools: request.host_tools.clone(),
        });

        let run = runtime::execute_run(runtime::ExecuteRunArgs {
            run_id: run_id.clone(),
            session_id: session_id.clone(),
            session,
            initial_input: request.input,
            factory,
            on_complete: Some(on_complete),
        });

        // Register while holding the same map lock used by on_complete. The
        // completion token is cancelled before that callback takes the lock, so
        // this ordering closes the check/insert race that could leave a finished
        // run registered forever.
        let mut active_runs = self.active_runs.lock();
        if !completed.is_cancelled() {
            active_runs.insert(session_id, ActiveRun {
                run_id,
                handle: run.handle(),
                completed,
            });
        }
        drop(active_runs);

        Ok(run)
    }

    // -- fork ----------------------------------------------------------------

    /// Fork an independent, non-persisted agent for side conversations.
    pub fn fork(self: &Arc<Self>, request: ForkRequest) -> Result<ForkedAgent> {
        let Self {
            llm,
            system_prompt: _,
            system_prompt_sections: _,
            limits,
            skills_dirs: _,
            skill_names: _,
            cwd,
            spill_root: _,
            storage: _,
            variables: _,
            sandbox,
            provider_override: _,
            active_runs: _,
            new_session_llm: _,
        } = self.as_ref();

        let forked = Arc::new(Self {
            llm: RwLock::new(llm.read().clone()),
            system_prompt: RwLock::new(request.system_prompt),
            system_prompt_sections: RwLock::new(Vec::new()),
            limits: RwLock::new(limits.read().clone()),
            skills_dirs: RwLock::new(vec![]),
            skill_names: RwLock::new(None),
            cwd: cwd.clone(),
            spill_root: None,
            storage: RwLock::new(Arc::new(MemoryStorage::new())),
            variables: RwLock::new(None),
            sandbox: super::sandbox::SandboxPolicy {
                enabled: sandbox.enabled,
                extra_dirs: sandbox.extra_dirs.clone(),
            },
            provider_override: RwLock::new(None),
            active_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            new_session_llm: None,
        });
        Ok(ForkedAgent {
            agent: forked,
            session_id: None,
        })
    }

    // -- session queries -----------------------------------------------------

    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<SessionMeta>> {
        let storage = self.storage.read().clone();
        storage
            .list_sessions(ListSessions { limit, offset: 0 })
            .await
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

    pub async fn delete_session(&self, session_id: &str) -> Result<bool> {
        let storage = self.storage.read().clone();
        storage.delete_session(session_id).await
    }

    pub async fn list_favorites(&self) -> Result<Vec<String>> {
        let storage = self.storage.read().clone();
        storage.load_favorites().await
    }

    /// Remove deleted ids from the favorites document. Returns how many favorite
    /// entries were pruned.
    pub async fn remove_favorites(&self, session_ids: &[String]) -> Result<usize> {
        let storage = self.storage.read().clone();
        let ids = storage.load_favorites().await?;
        let before = ids.len();
        let kept: Vec<String> = ids
            .into_iter()
            .filter(|id| !session_ids.iter().any(|deleted| deleted == id))
            .collect();
        let removed = before.saturating_sub(kept.len());
        if removed > 0 {
            storage.save_favorites(kept).await?;
        }
        Ok(removed)
    }

    /// Toggle a session's favorite state, returning the new state (`true` =
    /// now favorited). Persisted via the storage backend's favorites document.
    pub async fn toggle_favorite(&self, session_id: &str) -> Result<bool> {
        let storage = self.storage.read().clone();
        let mut ids = storage.load_favorites().await?;
        let now_favorited = if let Some(pos) = ids.iter().position(|id| id == session_id) {
            ids.remove(pos);
            false
        } else {
            ids.push(session_id.to_string());
            true
        };
        storage.save_favorites(ids).await?;
        Ok(now_favorited)
    }

    pub async fn create_session(&self, source: &str) -> Result<SessionMeta> {
        self.create_session_in(source, None).await
    }

    /// Create a blank session, optionally bound to an explicit workspace.
    pub async fn create_session_in(
        &self,
        source: &str,
        cwd: Option<String>,
    ) -> Result<SessionMeta> {
        self.create_session_with_llm(source, cwd, None).await
    }

    /// Create a blank session pinned to an explicit (provider, model).
    async fn create_session_with_llm(
        &self,
        source: &str,
        cwd: Option<String>,
        llm: Option<(String, String)>,
    ) -> Result<SessionMeta> {
        let (provider, model) = match llm {
            Some(pair) => pair,
            None => {
                // Promote the Premium landing so session and runs agree.
                let live = self.llm.read().clone();
                match self.new_session_llm.as_ref() {
                    Some(p) if p.provider != live.provider || p.model != live.model => {
                        *self.llm.write() = p.clone();
                        (p.provider.clone(), p.model.clone())
                    }
                    _ => (live.provider.clone(), live.model.clone()),
                }
            }
        };
        let storage = self.storage.read().clone();
        let id = crate::types::new_id();
        let dir = match cwd {
            Some(path) => canonical_workspace(&path)?,
            None => self.cwd.clone(),
        };
        let session =
            Session::new_with_provider_source(id, dir, provider, model, source, storage).await?;
        Ok(session.meta().await)
    }

    pub async fn load_transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        let storage = self.storage.read().clone();
        if storage.get_session(id).await?.is_none() {
            return Ok(Vec::new());
        }
        let entries = storage
            .list_entries(ListTranscriptEntries {
                session_id: id.to_string(),
                run_id: None,
                after_seq: None,
                limit: None,
            })
            .await?;
        Ok(entries.into_iter().map(|entry| entry.item).collect())
    }

    pub async fn load_context_transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        let storage = self.storage.read().clone();
        match Session::open(id, storage).await? {
            Some(session) => Ok(session.transcript().await),
            None => Ok(Vec::new()),
        }
    }

    /// Load the current branch for terminal replay without serializing all
    /// superseded history or the Engine-only payload embedded in a compact
    /// snapshot. Retained messages are replayed before a lightweight compact
    /// card, followed by entries written after that compact point.
    pub async fn load_resume_transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        let storage = self.storage.read().clone();
        if storage.get_session(id).await?.is_none() {
            return Ok(Vec::new());
        }
        let entries = storage.load_active_entries(id).await?;
        Ok(resume_transcript_items(entries))
    }

    pub async fn load_session(&self, id: &str) -> Result<Option<Arc<Session>>> {
        let storage = self.storage.read().clone();
        Session::open(id, storage).await
    }

    // -- private -------------------------------------------------------------

    fn build_system_prompt(&self, mode: ToolMode, cwd: &str) -> (String, Vec<Section>) {
        let mut sections = self.system_prompt_sections.read().clone();
        bind_workspace_sections(&mut sections, cwd);

        let ctx = DynamicContext {
            mode: prompt_mode(mode),
            sandbox: self.sandbox.enabled,
            variables: self
                .variables
                .read()
                .as_ref()
                .map(|v| v.variable_names())
                .unwrap_or_default(),
        };
        sections.extend(dynamic_sections(&ctx));

        let text = sections
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        (text, sections)
    }

    /// Search recent sessions for literal matches before falling back to
    /// semantic ranking with the configured LLM.
    async fn handle_resume_search(&self, query: &str) -> Result<String> {
        let storage = self.storage.read().clone();
        let sessions = storage
            .list_sessions_with_text(crate::agent::resume_search::SESSION_LIMIT)
            .await?;
        if let Some(results) = crate::agent::resume_search::literal_results(query, &sessions) {
            return Ok(results);
        }

        let llm = self.llm.read().clone();
        if llm.provider.is_empty() || llm.api_key.trim().is_empty() {
            return Err(EvotError::Conf(
                "Semantic session search needs a configured LLM provider.".to_string(),
            ));
        }
        let ctx = crate::agent::resume_search::RankContext {
            provider: self.llm_provider(&llm.protocol),
            llm,
        };
        crate::agent::resume_search::rank_sessions(&ctx, query, &sessions).await
    }

    /// Build a structured snapshot of what evot would send to the LLM right
    /// now (system prompt + tool definitions). Persists
    /// to JSON and returns a human-readable status string.
    async fn handle_dump_command(
        self: &Arc<Self>,
        mode: ToolMode,
        session: &Arc<Session>,
        target: Option<&str>,
    ) -> Result<String> {
        let session_id = session.session_id().await;
        // build_turn runs the full per-turn assembly (tools, skills).
        let llm = self.llm();
        let turn = self
            .build_turn(
                &llm,
                mode,
                Arc::clone(session),
                &session_id,
                Vec::new(),
                None,
            )
            .await?;

        let dump = build_prompt_dump(mode, &turn);

        let path = resolve_dump_path(target)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                EvotError::Agent(format!(
                    "failed to create dump dir {}: {err}",
                    parent.display()
                ))
            })?;
        }
        let json = serde_json::to_string_pretty(&dump)
            .map_err(|err| EvotError::Agent(format!("failed to serialize prompt dump: {err}")))?;
        std::fs::write(&path, json).map_err(|err| {
            EvotError::Agent(format!("failed to write dump to {}: {err}", path.display()))
        })?;

        Ok(format!(
            "Prompt dump saved to {}\n  system_prompt: {} tokens ({} sections)\n  tools: {} entries, {} tokens\n  total: {} tokens",
            path.display(),
            dump.totals.system_prompt_tokens,
            dump.system_prompt.sections.len(),
            dump.tools.len(),
            dump.totals.tool_definition_tokens,
            dump.totals.grand_total,
        ))
    }

    async fn resolve_session(
        &self,
        session_id: Option<&str>,
        source: &str,
        llm: &LlmConfig,
        cwd: Option<&str>,
    ) -> Result<Arc<Session>> {
        let provider = llm.provider.clone();
        let model = llm.model.clone();
        let thinking_level = Self::persisted_thinking_level_for(llm);
        let storage = self.storage.read().clone();
        let session = match session_id {
            Some(id) => match Session::open(id, storage.clone()).await? {
                Some(session) => {
                    session.set_model_selection(provider, model).await?;
                    session
                }
                None => {
                    Session::new_with_provider_source(
                        id.to_string(),
                        Self::create_cwd(cwd, &self.cwd)?,
                        provider,
                        model,
                        source,
                        storage,
                    )
                    .await?
                }
            },
            None => {
                let id = crate::types::new_id();
                Session::new_with_provider_source(
                    id,
                    Self::create_cwd(cwd, &self.cwd)?,
                    provider,
                    model,
                    source,
                    storage,
                )
                .await?
            }
        };
        // Mirror the live model selection: every run stamps the session with the
        // agent's current reasoning effort so it survives restarts (persisted by
        // the run's final `save()`).
        session.set_thinking_level(thinking_level).await;

        Ok(session)
    }

    /// The session-facing label for the agent's current thinking level, or
    /// `None` when the level is not a selectable tier for the active model
    /// (e.g. a config-set level the model rejects). Resume restores this
    /// snapshot over the config default, so gating on membership keeps the
    /// metadata meaningful.
    fn persisted_thinking_level_for(llm: &LlmConfig) -> Option<String> {
        let level = llm.thinking_level;
        if Self::supported_thinking_levels_for(llm).contains(&level) {
            Some(level.as_str().to_string())
        } else {
            None
        }
    }

    fn create_cwd(requested: Option<&str>, fallback: &str) -> Result<String> {
        match requested {
            Some(path) => canonical_workspace(path),
            None => Ok(fallback.to_string()),
        }
    }

    async fn build_turn(
        &self,
        llm: &LlmConfig,
        mode: ToolMode,
        session: Arc<Session>,
        session_id: &str,
        input: Vec<evot_engine::Content>,
        host_tools: Option<HostTools>,
    ) -> Result<runtime::TurnInput> {
        let llm = llm.clone();
        if llm.provider.is_empty() {
            return Err(EvotError::Conf(
                "No model available yet. Log in via the dashboard sidebar, run `evot login` here, or add a provider on the Models page."
                    .to_string(),
            ));
        }
        if llm.api_key.trim().is_empty() {
            return Err(EvotError::Conf(format!(
                "No API key set for provider '{}'. Add it in the dashboard settings \
                 or set EVOT_LLM_{}_API_KEY in your env file.",
                llm.provider,
                llm.provider.to_uppercase().replace('-', "_"),
            )));
        }
        let envs = self
            .variables()
            .map(|v| v.all_env_pairs())
            .unwrap_or_default();
        // Build path guard from sandbox policy. System dirs cover skill scan
        // directories plus the memory vault used by the builtin memory skill.
        let cwd = session.meta().await.cwd;
        let cwd_path = std::path::Path::new(&cwd);
        let skill_dirs = self.skills_dirs.read().clone();
        let selected_skill_names = self.skill_names.read().clone();
        let skills = load_turn_skills(&skill_dirs, selected_skill_names.as_deref())?;
        let mut system_dirs = skill_dirs.clone();
        if let Ok(memory_dir) = crate::conf::paths::memory_dir() {
            if let Err(e) = std::fs::create_dir_all(&memory_dir) {
                tracing::warn!("cannot create memory dir {}: {e}", memory_dir.display());
            }
            system_dirs.push(memory_dir);
        }
        for skill in &skills {
            system_dirs.push(skill.base_dir.clone());
        }
        let sandbox_rt = self.sandbox.build_runtime(cwd_path, &system_dirs)?;

        let tools = build_tools(
            mode,
            envs,
            sandbox_rt.allow_bash,
            sandbox_rt.bash_sandbox_dirs,
            host_tools,
        );

        let (mut system_prompt, mut sections) = self.build_system_prompt(mode, &cwd);
        if let Some(section) = skills_prompt_section(&skills) {
            let insert_at = sections
                .iter()
                .position(|section| matches!(section.name, "environment" | "dynamic_boundary"))
                .unwrap_or(sections.len());
            sections.insert(insert_at, section);
            system_prompt = sections
                .iter()
                .map(|section| section.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
        }

        let (prior_messages, compaction_state, transcript_seq) = session.context_snapshot().await;
        let prior_messages = evot_engine::sanitize_tool_pairs(prior_messages);

        Ok(runtime::TurnInput {
            options: runtime::EngineOptions {
                provider: llm.provider,
                protocol: llm.protocol,
                model: llm.model,
                api_key: llm.api_key,
                model_config: llm.model_config,
                system_prompt,
                system_prompt_sections: sections,
                limits: if mode.is_interactive() {
                    None
                } else {
                    Some(self.limits.read().clone())
                },
                tools,
                thinking_level: llm.thinking_level,
                cwd: cwd_path.to_path_buf(),
                path_guard: sandbox_rt.path_guard,
                spill_dir: self
                    .spill_root
                    .as_ref()
                    .map(|root| root.join("sessions").join(session_id).join("tool-results")),
                prompt_cache_key: Some(session_id.to_string()),
                provider_override: self.provider_override.read().clone(),
                compaction_state,
            },
            history: prior_messages,
            input,
            session,
            transcript_seq,
        })
    }
}

// ---------------------------------------------------------------------------
// AgentTurnFactory — bridges Agent's per-turn build to the runtime
// ---------------------------------------------------------------------------

struct AgentTurnFactory {
    agent: Arc<Agent>,
    session: Arc<Session>,
    mode: ToolMode,
    session_id: String,
    llm: LlmConfig,
    host_tools: Option<HostTools>,
}

#[async_trait::async_trait]
impl TurnFactory for AgentTurnFactory {
    async fn build(&self, input: Vec<evot_engine::Content>) -> Result<runtime::TurnInput> {
        self.agent
            .build_turn(
                &self.llm,
                self.mode,
                Arc::clone(&self.session),
                &self.session_id,
                input,
                self.host_tools.clone(),
            )
            .await
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_manual_compaction_outcome(
    outcome: &crate::compact::orchestrator::ManualCompactionOutcome,
) -> String {
    match outcome {
        crate::compact::orchestrator::ManualCompactionOutcome::Compacted {
            tokens_before,
            tokens_after,
            messages_before,
            messages_after,
            context_window,
            used_fallback,
            method,
            remote_blob_bytes,
            fallback_reason,
            ..
        } => {
            let mut line = format!(
                "Session compacted: {tokens_before} → {tokens_after} tokens, {messages_before} → {messages_after} messages."
            );
            if *used_fallback {
                line.push_str(
                    "\nNote: the LLM summary was unavailable; a deterministic fallback summary was used.",
                );
            }
            match method {
                Some(evot_engine::CompactionMethod::Remote) => {
                    line.push_str("\nProvider-native remote compaction was used.");
                    if let Some(bytes) = remote_blob_bytes {
                        line.push_str(&format!(" Native blob: {bytes} bytes."));
                    }
                }
                Some(evot_engine::CompactionMethod::RemoteFailedLocal) => {
                    line.push_str(
                        "\nProvider-native remote compaction failed; local summarization was used.",
                    );
                    if let Some(reason) = fallback_reason {
                        line.push_str(&format!(" Reason: {reason}"));
                    }
                }
                Some(evot_engine::CompactionMethod::Local) if fallback_reason.is_some() => {
                    if let Some(reason) = fallback_reason {
                        line.push_str(&format!(
                            "\nProvider-native remote compaction was unavailable; local summarization was used. Reason: {reason}"
                        ));
                    }
                }
                _ => {}
            }
            if *context_window > 0 && tokens_after >= context_window {
                line.push_str(&format!(
                    "\nWarning: context is still {tokens_after} tokens, above this model's {context_window}-token window. \
                     Switch to a larger-context model or start a new session to continue."
                ));
            }
            line
        }
        crate::compact::orchestrator::ManualCompactionOutcome::NothingToCompact => {
            "Nothing to compact.".into()
        }
        crate::compact::orchestrator::ManualCompactionOutcome::Cancelled => {
            "Compaction cancelled.".into()
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt dump helpers
// ---------------------------------------------------------------------------

/// Conservative whitespace-based proxy for token count. Avoids a tokenizer
/// dependency in the dump path — for prompt-budget sanity checks it's fine,
/// and replay tooling can re-tokenize the text directly. Roughly
/// `len / 4` is the rule of thumb.
fn rough_tokens(s: &str) -> usize {
    let chars = s.chars().count();
    chars.div_ceil(4)
}

fn mode_label(mode: ToolMode) -> &'static str {
    match mode {
        ToolMode::Interactive => "Interactive",
        ToolMode::Headless => "Headless",
        ToolMode::Planning => "Planning",
        ToolMode::Readonly => "Readonly",
    }
}

/// Distil the runtime [`ToolMode`] into the prompt-layer [`PromptMode`].
fn resume_transcript_items(entries: Vec<TranscriptEntry>) -> Vec<TranscriptItem> {
    let mut items = Vec::new();
    for entry in entries {
        match entry.item {
            TranscriptItem::Compact {
                id,
                created_at,
                reason,
                summary,
                tokens_before,
                tokens_after,
                messages_before,
                messages_after,
                messages,
                details,
                ..
            } => {
                let skip = usize::from(messages.first().is_some_and(|message| {
                    matches!(
                        message,
                        TranscriptItem::User { text, .. }
                            if crate::compact::context_view::is_summary_boundary_text(text, &summary)
                    )
                }));
                items.extend(
                    messages
                        .into_iter()
                        .skip(skip)
                        .filter(TranscriptItem::is_context_item),
                );
                items.push(TranscriptItem::Compact {
                    id,
                    created_at,
                    reason,
                    summary: evot_engine::truncate_summary(
                        &summary,
                        evot_engine::context::DEFAULT_SUMMARY_MAX_BYTES,
                    ),
                    tokens_before,
                    tokens_after,
                    messages_before,
                    messages_after,
                    messages: Vec::new(),
                    engine_messages: Vec::new(),
                    state: Box::default(),
                    details,
                });
            }
            TranscriptItem::Marker { messages, .. } => {
                items.extend(messages.into_iter().filter(TranscriptItem::is_context_item));
            }
            item => items.push(item),
        }
    }
    items
}

fn load_turn_skills(dirs: &[PathBuf], names: Option<&[String]>) -> Result<Vec<SkillSpec>> {
    match names {
        Some(names) => crate::agent::prompt::skill::load_skills_by_name(dirs, names),
        None => crate::agent::prompt::skill::load_skills(dirs),
    }
    .map_err(|error| EvotError::Agent(format!("failed to load skills: {error}")))
}

fn skills_prompt_section(skills: &[SkillSpec]) -> Option<Section> {
    let text = format_skills_for_prompt(skills);
    if text.is_empty() {
        None
    } else {
        Some(Section {
            name: "skills",
            text,
        })
    }
}

fn prompt_mode(mode: ToolMode) -> PromptMode {
    match mode {
        ToolMode::Interactive => PromptMode::Interactive,
        ToolMode::Planning => PromptMode::Planning,
        ToolMode::Headless => PromptMode::Headless,
        ToolMode::Readonly => PromptMode::Readonly,
    }
}

fn build_prompt_dump(mode: ToolMode, turn: &runtime::TurnInput) -> PromptDump {
    let opts = &turn.options;

    // System prompt sections — sourced from the turn (includes planning,
    // variables, sandbox, skills). Falls back to a single section if empty.
    let section_dumps = if opts.system_prompt_sections.is_empty() {
        vec![SectionDump {
            name: "system_prompt".into(),
            text: opts.system_prompt.clone(),
            tokens: rough_tokens(&opts.system_prompt),
        }]
    } else {
        opts.system_prompt_sections
            .iter()
            .map(|s| SectionDump {
                name: s.name.to_string(),
                text: s.text.clone(),
                tokens: rough_tokens(&s.text),
            })
            .collect()
    };

    let system_tokens = rough_tokens(&opts.system_prompt);
    let system_prompt = SystemPromptDump {
        text: opts.system_prompt.clone(),
        tokens: system_tokens,
        sections: section_dumps,
    };

    // Tool definitions
    let mut tool_dumps: Vec<ToolDump> = opts
        .tools
        .iter()
        .map(|t| {
            let name = t.name().to_string();
            let description = t.description().to_string();
            let parameters = t.parameters_schema();
            let serialized = format!("{name}\n{description}\n{parameters}");
            ToolDump {
                name,
                description,
                parameters,
                tokens: rough_tokens(&serialized),
            }
        })
        .collect();
    tool_dumps.sort_by(|a, b| a.name.cmp(&b.name));
    let tool_tokens: usize = tool_dumps.iter().map(|t| t.tokens).sum();

    PromptDump {
        evot_version: env!("CARGO_PKG_VERSION").to_string(),
        cwd: opts.cwd.display().to_string(),
        mode: mode_label(mode).into(),
        model: opts.model.clone(),
        thinking_level: opts.thinking_level.as_str().into(),
        system_prompt,
        tools: tool_dumps,
        totals: TokenTotals {
            system_prompt_tokens: system_tokens,
            tool_definition_tokens: tool_tokens,
            grand_total: system_tokens + tool_tokens,
        },
    }
}

fn resolve_dump_path(target: Option<&str>) -> Result<PathBuf> {
    if let Some(t) = target {
        return Ok(PathBuf::from(t));
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| EvotError::Agent("HOME not set; cannot pick default dump path".into()))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    Ok(PathBuf::from(home)
        .join(".evotai")
        .join("dumps")
        .join(format!("prompt-{stamp}.json")))
}

fn canonical_workspace(cwd: &str) -> Result<String> {
    let path = crate::conf::paths::expand_home_path(cwd.trim())?;
    if path.as_os_str().is_empty() {
        return Err(EvotError::Conf("workspace path must not be empty".into()));
    }
    let metadata = std::fs::metadata(&path).map_err(|error| {
        EvotError::Conf(format!(
            "workspace '{}' is not accessible: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(EvotError::Conf(format!(
            "workspace '{}' is not a directory",
            path.display()
        )));
    }
    let canonical = std::fs::canonicalize(&path).unwrap_or(path);
    Ok(canonical.to_string_lossy().into_owned())
}

fn bind_workspace_sections(sections: &mut Vec<Section>, cwd: &str) {
    use crate::agent::prompt::SystemPrompt;
    for section in sections.iter_mut() {
        match section.name {
            "environment" => section.text = SystemPrompt::environment_text(cwd),
            "project_context" => {
                section.text = SystemPrompt::project_context_text(cwd).unwrap_or_default();
            }
            _ => {}
        }
    }
    if !sections.iter().any(|section| section.name == "environment") {
        let insert_at = sections
            .iter()
            .position(|section| section.name == "dynamic_boundary")
            .unwrap_or(sections.len());
        sections.insert(insert_at, Section {
            name: "environment",
            text: SystemPrompt::environment_text(cwd),
        });
    }
    if let Some(text) = SystemPrompt::project_context_text(cwd) {
        if let Some(section) = sections
            .iter_mut()
            .find(|section| section.name == "project_context")
        {
            if section.text.is_empty() {
                section.text = text;
            }
        } else {
            let insert_at = sections
                .iter()
                .position(|section| matches!(section.name, "environment" | "dynamic_boundary"))
                .unwrap_or(sections.len());
            sections.insert(insert_at, Section {
                name: "project_context",
                text,
            });
        }
    }
    sections.retain(|section| !(section.name == "project_context" && section.text.is_empty()));
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
            SubmitOutcome::Command(_) => Err(EvotError::Run(
                "commands not supported in forked agent".into(),
            )),
        }
    }
}
