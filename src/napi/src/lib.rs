use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// NapiAgent — wraps the app-level Agent for JS consumption
// ---------------------------------------------------------------------------

#[napi]
pub struct NapiAgent {
    agent: Arc<bendclaw::agent::Agent>,
}

#[napi]
impl NapiAgent {
    /// Load config from disk and create an agent.
    /// Optional `model` override.
    #[napi(factory)]
    pub fn create(model: Option<String>) -> Result<Self> {
        let config = bendclaw::conf::Config::load()
            .map_err(|e| Error::from_reason(format!("config load failed: {e}")))?
            .with_model(model);

        let cwd = std::env::current_dir()
            .map_err(|e| Error::from_reason(format!("cwd: {e}")))?
            .to_string_lossy()
            .to_string();

        let system_prompt = bendclaw::agent::prompt::SystemPrompt::new(&cwd)
            .with_system()
            .with_git()
            .with_tools()
            .with_project_context()
            .with_memory()
            .with_claude_memory()
            .build();

        let agent = bendclaw::agent::Agent::new(&config, &cwd)
            .map_err(|e| Error::from_reason(format!("agent init: {e}")))?
            .with_system_prompt(system_prompt)
            .with_skills_dirs(build_skills_dirs());

        // Load variables
        let rt = tokio::runtime::Handle::current();
        let storage = agent.storage();
        let records = rt.block_on(storage.load_variables()).unwrap_or_default();
        let variables = Arc::new(bendclaw::agent::Variables::new(storage, records));
        agent.with_variables(variables);

        Ok(Self { agent })
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
    /// Returns a NapiQueryStream that can be iterated with `next()`.
    #[napi]
    pub async fn query(
        &self,
        prompt: String,
        session_id: Option<String>,
    ) -> Result<NapiQueryStream> {
        let request = bendclaw::agent::QueryRequest::text(prompt).session_id(session_id);

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
        })
    }

    /// List recent sessions.
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
}

// ---------------------------------------------------------------------------
// NapiQueryStream — async iterator over RunEvents
// ---------------------------------------------------------------------------

#[napi]
pub struct NapiQueryStream {
    inner: Mutex<bendclaw::agent::QueryStream>,
    cached_session_id: String,
    aborted: Arc<AtomicBool>,
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
        match stream.next().await {
            Some(event) => {
                let json = serde_json::to_string(&event)
                    .map_err(|e| Error::from_reason(format!("serialize event: {e}")))?;
                Ok(Some(json))
            }
            None => Ok(None),
        }
    }

    /// Abort the running query. Safe to call while next() is awaiting.
    #[napi]
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
        // If we can grab the lock, abort the engine immediately.
        // Otherwise, the next() call will see the aborted flag and return None.
        if let Ok(stream) = self.inner.try_lock() {
            stream.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_skills_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(global) = bendclaw::conf::paths::skills_dir() {
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
    let mut config = bendclaw::conf::Config::load()
        .map_err(|e| Error::from_reason(format!("config load failed: {e}")))?
        .with_model(model);
    if let Some(p) = port {
        config = config.with_port(p);
    }
    bendclaw::server::start(config)
        .await
        .map_err(|e| Error::from_reason(format!("server error: {e}")))
}
