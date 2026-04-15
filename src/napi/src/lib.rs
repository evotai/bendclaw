use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use evot::agent::Agent;
use evot::agent::ForkRequest;
use evot::agent::ForkedAgent;
use evot::agent::QueryRequest;
use evot::agent::ToolMode;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use tokio::sync::Mutex;
use tokio::sync::Notify;

// ---------------------------------------------------------------------------
// NapiAgent — wraps the app-level Agent for JS consumption
// ---------------------------------------------------------------------------

#[napi]
pub struct NapiAgent {
    agent: Arc<Agent>,
    config: evot::conf::Config,
}

#[napi]
impl NapiAgent {
    /// Load config from disk and create an agent.
    /// Optional `model` override.
    #[napi(factory)]
    pub fn create(model: Option<String>) -> Result<Self> {
        let config = evot::conf::Config::load()
            .map_err(|e| Error::from_reason(format!("config load failed: {e}")))?
            .with_model(model);

        let cwd = std::env::current_dir()
            .map_err(|e| Error::from_reason(format!("cwd: {e}")))?
            .to_string_lossy()
            .to_string();

        let system_prompt = evot::agent::prompt::SystemPrompt::new(&cwd)
            .with_agent_behavior()
            .with_system()
            .with_git()
            .with_tools()
            .with_project_context()
            .with_memory()
            .with_claude_memory()
            .build();

        let agent = evot::agent::Agent::new(&config, &cwd)
            .map_err(|e| Error::from_reason(format!("agent init: {e}")))?
            .with_system_prompt(system_prompt)
            .with_skills_dirs(build_skills_dirs());

        // Load variables
        let rt = tokio::runtime::Handle::current();
        let storage = agent.storage();
        let records = rt.block_on(storage.load_variables()).unwrap_or_default();
        let variables = Arc::new(evot::agent::Variables::new(storage, records));
        agent.with_variables(variables);

        Ok(Self { agent, config })
    }

    /// Current model name.
    #[napi(getter)]
    pub fn model(&self) -> String {
        self.agent.llm().model
    }

    /// Set the active model.
    #[napi(setter)]
    pub fn set_model(&mut self, model: String) {
        self.agent.set_model(model);
    }

    /// Current working directory.
    #[napi(getter)]
    pub fn cwd(&self) -> String {
        self.agent.cwd().to_string()
    }

    /// Send a prompt and get a stream of events.
    /// Optional tool_mode: "planning", "readonly", or omit for default (headless).
    #[napi]
    pub async fn query(
        &self,
        prompt: String,
        session_id: Option<String>,
        tool_mode: Option<String>,
    ) -> Result<NapiQueryStream> {
        let mode = match tool_mode.as_deref() {
            Some("planning") => ToolMode::Planning { ask_fn: None },
            Some("readonly") => ToolMode::Readonly,
            _ => ToolMode::Headless,
        };
        let request = QueryRequest::text(prompt).session_id(session_id).mode(mode);

        let stream = self
            .agent
            .query(request)
            .await
            .map_err(|e| Error::from_reason(format!("query failed: {e}")))?;

        let sid = stream.session_id.clone();

        Ok(NapiQueryStream {
            inner: Mutex::new(stream),
            cached_session_id: sid,
            aborted: Arc::new(AtomicBool::new(false)),
            abort_notify: Arc::new(Notify::new()),
        })
    }

    #[napi]
    pub async fn list_sessions(&self, limit: Option<u32>) -> Result<String> {
        let sessions = self
            .agent
            .list_sessions(limit.unwrap_or(20) as usize)
            .await
            .map_err(|e| Error::from_reason(format!("list sessions: {e}")))?;

        serde_json::to_string(&sessions).map_err(|e| Error::from_reason(format!("serialize: {e}")))
    }

    /// Load transcript for a session.
    #[napi]
    pub async fn load_transcript(&self, session_id: String) -> Result<String> {
        let items = self
            .agent
            .load_transcript(&session_id)
            .await
            .map_err(|e| Error::from_reason(format!("load transcript: {e}")))?;

        serde_json::to_string(&items).map_err(|e| Error::from_reason(format!("serialize: {e}")))
    }

    /// Fork the agent for a side conversation (readonly, ephemeral).
    #[napi]
    pub fn fork(&self, system_prompt: String) -> Result<NapiForkedAgent> {
        let request = ForkRequest { system_prompt };
        let forked = self
            .agent
            .fork(request)
            .map_err(|e| Error::from_reason(format!("fork: {e}")))?;
        Ok(NapiForkedAgent {
            inner: Arc::new(Mutex::new(forked)),
        })
    }

    /// List agent variables as JSON array of {key, value}.
    #[napi]
    pub fn list_variables(&self) -> Result<String> {
        match self.agent.variables() {
            Some(vars) => {
                let items: Vec<_> = vars
                    .list_global()
                    .iter()
                    .map(|v| serde_json::json!({ "key": v.key, "value": v.value }))
                    .collect();
                serde_json::to_string(&items)
                    .map_err(|e| Error::from_reason(format!("serialize: {e}")))
            }
            None => Ok("[]".to_string()),
        }
    }

    /// Set an agent variable (persisted).
    #[napi]
    pub async fn set_variable(&self, key: String, value: String) -> Result<()> {
        match self.agent.variables() {
            Some(vars) => vars
                .set_global(key, value)
                .await
                .map_err(|e| Error::from_reason(format!("set variable: {e}"))),
            None => Err(Error::from_reason("variables not available")),
        }
    }

    /// Delete an agent variable. Returns true if it existed.
    #[napi]
    pub async fn delete_variable(&self, key: String) -> Result<bool> {
        match self.agent.variables() {
            Some(vars) => vars
                .delete_global(&key)
                .await
                .map_err(|e| Error::from_reason(format!("delete variable: {e}"))),
            None => Err(Error::from_reason("variables not available")),
        }
    }

    /// Get config info: provider, env path, base URL, configured models.
    #[napi]
    pub fn config_info(&self) -> Result<String> {
        let llm = self.agent.llm();
        let provider = format!("{}", self.config.llm.provider);
        let env_path = evot::conf::paths::env_file_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let available = self.collect_models();
        let info = serde_json::json!({
            "provider": provider,
            "envPath": env_path,
            "baseUrl": llm.base_url,
            "anthropicModel": self.config.anthropic.model,
            "openaiModel": self.config.openai.model,
            "availableModels": available,
            "thinkingLevel": format!("{:?}", self.config.llm.thinking_level).to_lowercase(),
        });
        serde_json::to_string(&info).map_err(|e| Error::from_reason(format!("serialize: {e}")))
    }

    /// Get the list of available models from config (unique, non-empty).
    #[napi]
    pub fn available_models(&self) -> Vec<String> {
        self.collect_models()
    }

    fn collect_models(&self) -> Vec<String> {
        let llm = self.agent.llm();
        let mut models = Vec::new();
        for m in [
            &self.config.anthropic.model,
            &self.config.openai.model,
            &llm.model,
        ] {
            let trimmed = m.trim();
            if !trimmed.is_empty() && !models.contains(&trimmed.to_string()) {
                models.push(trimmed.to_string());
            }
        }
        models
    }

    /// Switch the active provider ("anthropic" or "openai") and update the LLM config.
    #[napi]
    pub fn set_provider(&self, provider: String) -> Result<()> {
        let kind = evot::conf::ProviderKind::from_str_loose(&provider)
            .map_err(|e| Error::from_reason(format!("invalid provider: {e}")))?;
        self.agent.set_provider(kind.clone());
        let llm = match kind {
            evot::conf::ProviderKind::Anthropic => evot::conf::LlmConfig {
                provider: kind,
                api_key: self.config.anthropic.api_key.clone(),
                base_url: self.config.anthropic.base_url.clone(),
                model: self.config.anthropic.model.clone(),
                thinking_level: self.config.llm.thinking_level,
            },
            evot::conf::ProviderKind::OpenAi => evot::conf::LlmConfig {
                provider: kind,
                api_key: self.config.openai.api_key.clone(),
                base_url: self.config.openai.base_url.clone(),
                model: self.config.openai.model.clone(),
                thinking_level: self.config.llm.thinking_level,
            },
        };
        self.agent.set_llm(llm);
        Ok(())
    }

    /// Set execution limits (max turns, tokens, duration).
    #[napi]
    pub fn set_limits(
        &self,
        max_turns: Option<u32>,
        max_tokens: Option<f64>,
        max_duration_secs: Option<f64>,
    ) {
        let mut limits = self.agent.limits();
        if let Some(t) = max_turns {
            limits.max_turns = t;
        }
        if let Some(t) = max_tokens {
            limits.max_total_tokens = t as u64;
        }
        if let Some(d) = max_duration_secs {
            limits.max_duration_secs = d as u64;
        }
        self.agent.with_limits(limits);
    }

    /// Append extra text to the system prompt.
    #[napi]
    pub fn append_system_prompt(&self, extra: String) {
        self.agent.append_system_prompt(&extra);
    }

    /// Add additional skills directories.
    #[napi]
    pub fn add_skills_dirs(&self, dirs: Vec<String>) {
        let paths: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
        self.agent.with_skills_dirs(paths);
    }
}

// ---------------------------------------------------------------------------
// NapiQueryStream — async iterator over RunEvents
// ---------------------------------------------------------------------------

#[napi]
pub struct NapiQueryStream {
    inner: Mutex<evot::agent::QueryStream>,
    cached_session_id: String,
    aborted: Arc<AtomicBool>,
    abort_notify: Arc<Notify>,
}

#[napi]
impl NapiQueryStream {
    /// Get the session ID for this query.
    #[napi(getter)]
    pub fn session_id(&self) -> String {
        self.cached_session_id.clone()
    }

    /// Get the next event as JSON. Returns null when the stream is done.
    #[napi]
    pub async fn next(&self) -> Result<Option<String>> {
        if self.aborted.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let mut stream = self.inner.lock().await;
        // Race between the next stream event and the abort signal so that
        // abort() can wake us up even while we're blocked on rx.recv().
        tokio::select! {
            event = stream.next() => {
                if self.aborted.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                match event {
                    Some(event) => {
                        let json = serde_json::to_string(&event)
                            .map_err(|e| Error::from_reason(format!("serialize event: {e}")))?;
                        Ok(Some(json))
                    }
                    None => Ok(None),
                }
            }
            _ = self.abort_notify.notified() => {
                stream.abort();
                Ok(None)
            }
        }
    }

    /// Abort the running query. Safe to call while next() is awaiting.
    #[napi]
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
        // Wake up any in-flight next() call so it returns None immediately.
        self.abort_notify.notify_waiters();
        // If we can grab the lock, abort the engine immediately too.
        if let Ok(stream) = self.inner.try_lock() {
            stream.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// NapiForkedAgent — ephemeral readonly side conversation
// ---------------------------------------------------------------------------

#[napi]
pub struct NapiForkedAgent {
    inner: Arc<Mutex<ForkedAgent>>,
}

#[napi]
impl NapiForkedAgent {
    /// Send a prompt to the forked agent. Returns a NapiQueryStream.
    #[napi]
    pub async fn query(&self, prompt: String) -> Result<NapiQueryStream> {
        let mut forked = self.inner.lock().await;
        let stream = forked
            .query(&prompt)
            .await
            .map_err(|e| Error::from_reason(format!("fork query: {e}")))?;
        let sid = stream.session_id.clone();
        Ok(NapiQueryStream {
            inner: Mutex::new(stream),
            cached_session_id: sid,
            aborted: Arc::new(AtomicBool::new(false)),
            abort_notify: Arc::new(Notify::new()),
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_skills_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(global) = evot::conf::paths::skills_dir() {
        dirs.push(global);
    }
    dirs
}

/// Version string for the native addon.
#[napi]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Start the HTTP server. Blocks until the server shuts down.
#[napi]
pub async fn start_server(port: Option<u16>, model: Option<String>) -> Result<()> {
    let mut config = evot::conf::Config::load()
        .map_err(|e| Error::from_reason(format!("config load failed: {e}")))?
        .with_model(model);
    if let Some(p) = port {
        config = config.with_port(p);
    }
    evot::gateway::service::start(config)
        .await
        .map_err(|e| Error::from_reason(format!("server error: {e}")))
}
