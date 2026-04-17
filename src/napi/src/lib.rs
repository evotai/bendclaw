use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use evot::agent::Agent;
use evot::agent::ForkRequest;
use evot::agent::ForkedAgent;
use evot::agent::QueryRequest;
use evot::agent::ToolMode;
use evot_engine::tools::AskUserFn;
use evot_engine::tools::AskUserRequest;
use evot_engine::tools::AskUserResponse;
use futures::FutureExt;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::Deserialize;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::sync::Notify;

/// Shared slot for the oneshot sender that unblocks the `AskUserFn` callback.
type AskResponder =
    Arc<Mutex<Option<oneshot::Sender<std::result::Result<AskUserResponse, String>>>>>;

/// Content block from JS — typed deserialization for queryWithContent.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum JsContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}
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
    pub fn create(model: Option<String>, env_file: Option<String>) -> Result<Self> {
        let config = evot::conf::Config::load_with_env_file(env_file.as_deref())
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
    /// Optional tool_mode: "interactive", "planning", "planning_interactive", "readonly",
    /// or omit for default (headless).
    ///
    /// When `content_json` is provided it takes precedence over `prompt`.
    /// Format: `[{"type":"text","text":"..."}, {"type":"image","data":"b64","mimeType":"image/png"}]`
    #[napi]
    pub async fn query(
        &self,
        prompt: String,
        session_id: Option<String>,
        tool_mode: Option<String>,
        content_json: Option<String>,
    ) -> Result<NapiRun> {
        // Channel for ask_user events injected into the event stream
        let (ask_event_tx, ask_event_rx) = tokio_mpsc::unbounded_channel::<String>();
        // Shared slot for the oneshot sender that unblocks the ask_user callback
        let ask_responder: AskResponder = Arc::new(Mutex::new(None));

        let mode = match tool_mode.as_deref() {
            Some("interactive") | Some("planning_interactive") => {
                let ask_fn = build_ask_fn(ask_event_tx, ask_responder.clone());
                if tool_mode.as_deref() == Some("planning_interactive") {
                    ToolMode::Planning {
                        ask_fn: Some(ask_fn),
                    }
                } else {
                    ToolMode::Interactive { ask_fn }
                }
            }
            Some("planning") => ToolMode::Planning { ask_fn: None },
            Some("readonly") => ToolMode::Readonly,
            _ => ToolMode::Headless,
        };

        let request = if let Some(json) = content_json {
            let blocks: Vec<JsContent> = serde_json::from_str(&json)
                .map_err(|e| Error::from_reason(format!("parse content: {e}")))?;

            let input: Vec<evot_engine::Content> = blocks
                .into_iter()
                .filter_map(|block| match block {
                    JsContent::Text { text } if !text.is_empty() => {
                        Some(evot_engine::Content::Text { text })
                    }
                    JsContent::Image { data, mime_type } if !data.is_empty() => {
                        Some(evot_engine::Content::Image { data, mime_type })
                    }
                    _ => None,
                })
                .collect();

            if input.is_empty() {
                return Err(Error::from_reason("empty content"));
            }

            QueryRequest::with_input(input)
                .session_id(session_id)
                .mode(mode)
                .source("repl")
        } else {
            QueryRequest::text(prompt)
                .session_id(session_id)
                .mode(mode)
                .source("repl")
        };

        let run = self
            .agent
            .query(request)
            .await
            .map_err(|e| Error::from_reason(format!("query failed: {e}")))?;

        let sid = run.session_id.clone();
        let handle = run.handle();

        Ok(NapiRun {
            inner: Mutex::new(run),
            handle,
            cached_session_id: sid,
            aborted: Arc::new(AtomicBool::new(false)),
            abort_notify: Arc::new(Notify::new()),
            ask_event_rx: Mutex::new(ask_event_rx),
            ask_responder,
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
        let env_path = evot::conf::paths::default_env_file_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let has_api_key = !llm.api_key.is_empty();
        let available = self.collect_models();
        let info = serde_json::json!({
            "provider": provider,
            "envPath": env_path,
            "hasApiKey": has_api_key,
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

    /// Send a steering message to the active run for a session.
    /// When `content_json` is provided, it takes precedence over `text`.
    #[napi]
    pub fn steer(&self, session_id: String, text: String, content_json: Option<String>) {
        let input = if let Some(json) = content_json {
            if let Ok(blocks) = serde_json::from_str::<Vec<JsContent>>(&json) {
                blocks
                    .into_iter()
                    .filter_map(|block| match block {
                        JsContent::Text { text } if !text.is_empty() => {
                            Some(evot_engine::Content::Text { text })
                        }
                        JsContent::Image { data, mime_type } if !data.is_empty() => {
                            Some(evot_engine::Content::Image { data, mime_type })
                        }
                        _ => None,
                    })
                    .collect()
            } else {
                vec![evot_engine::Content::Text { text }]
            }
        } else {
            vec![evot_engine::Content::Text { text }]
        };
        self.agent.steer(&session_id, input);
    }

    /// Send a follow-up message to the active run for a session.
    #[napi]
    pub fn follow_up(&self, session_id: String, text: String) {
        self.agent.follow_up(&session_id, &text);
    }

    /// Abort the active run for a session.
    #[napi]
    pub fn abort_run(&self, session_id: String) {
        self.agent.abort_run(&session_id);
    }
}

// ---------------------------------------------------------------------------
// NapiRun — async iterator over RunEvents
// ---------------------------------------------------------------------------

#[napi]
pub struct NapiRun {
    inner: Mutex<evot::agent::Run>,
    handle: evot_engine::RunHandle,
    cached_session_id: String,
    aborted: Arc<AtomicBool>,
    abort_notify: Arc<Notify>,
    /// Receives ask_user event JSON strings injected by the AskUserFn callback.
    ask_event_rx: Mutex<tokio_mpsc::UnboundedReceiver<String>>,
    /// Shared slot: the AskUserFn callback stores a oneshot::Sender here;
    /// `respond_ask_user()` takes it out and sends the answer.
    ask_responder: AskResponder,
}

#[napi]
impl NapiRun {
    /// Get the session ID for this run.
    #[napi(getter)]
    pub fn session_id(&self) -> String {
        self.cached_session_id.clone()
    }

    /// Get the next event as JSON. Returns null when the stream is done.
    ///
    /// When the agent calls `ask_user`, this will return an event with
    /// `kind: "ask_user"` containing the questions. The caller must then
    /// call `respondAskUser()` before calling `next()` again.
    #[napi]
    pub async fn next(&self) -> Result<Option<String>> {
        if self.aborted.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let mut run = self.inner.lock().await;
        let mut ask_rx = self.ask_event_rx.lock().await;
        tokio::select! {
            ask_json = ask_rx.recv() => {
                Ok(ask_json)
            }
            event = run.next() => {
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
                run.abort();
                Ok(None)
            }
        }
    }

    /// Respond to an `ask_user` event. Call this after receiving an event
    /// with `kind: "ask_user"`. Pass a JSON string representing the response:
    /// - `{"Answered":[{"header":"...","question":"...","answer":"..."},...]}`
    /// - `"Skipped"`
    #[napi]
    pub async fn respond_ask_user(&self, response_json: String) -> Result<()> {
        let response: AskUserResponse = serde_json::from_str(&response_json)
            .map_err(|e| Error::from_reason(format!("parse ask_user response: {e}")))?;
        let mut guard = self.ask_responder.lock().await;
        if let Some(tx) = guard.take() {
            let _ = tx.send(Ok(response));
        }
        Ok(())
    }

    /// Abort the running query. Safe to call while next() is awaiting.
    #[napi]
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
        self.abort_notify.notify_waiters();
        self.handle.abort();
    }

    /// Send a steering message into the running agent loop.
    /// When `content_json` is provided, it takes precedence over `text`.
    #[napi]
    pub fn steer(&self, text: String, content_json: Option<String>) {
        let content = if let Some(json) = content_json {
            if let Ok(blocks) = serde_json::from_str::<Vec<JsContent>>(&json) {
                blocks
                    .into_iter()
                    .filter_map(|block| match block {
                        JsContent::Text { text } if !text.is_empty() => {
                            Some(evot_engine::Content::Text { text })
                        }
                        JsContent::Image { data, mime_type } if !data.is_empty() => {
                            Some(evot_engine::Content::Image { data, mime_type })
                        }
                        _ => None,
                    })
                    .collect()
            } else {
                vec![evot_engine::Content::Text { text }]
            }
        } else {
            vec![evot_engine::Content::Text { text }]
        };
        self.handle
            .steer(evot_engine::AgentMessage::Llm(evot_engine::Message::User {
                content,
                timestamp: evot_engine::now_ms(),
            }));
    }

    /// Send a follow-up message (processed after current turn finishes).
    #[napi]
    pub fn follow_up(&self, text: String) {
        self.handle
            .follow_up(evot_engine::AgentMessage::Llm(evot_engine::Message::user(
                text,
            )));
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
    /// Send a prompt to the forked agent. Returns a NapiRun.
    #[napi]
    pub async fn query(&self, prompt: String) -> Result<NapiRun> {
        let mut forked = self.inner.lock().await;
        let run = forked
            .query(&prompt)
            .await
            .map_err(|e| Error::from_reason(format!("fork query: {e}")))?;
        let sid = run.session_id.clone();
        let handle = run.handle();
        // Forked agents are readonly — no ask_user support, use dummy channels
        let (_ask_tx, ask_rx) = tokio_mpsc::unbounded_channel::<String>();
        Ok(NapiRun {
            inner: Mutex::new(run),
            handle,
            cached_session_id: sid,
            aborted: Arc::new(AtomicBool::new(false)),
            abort_notify: Arc::new(Notify::new()),
            ask_event_rx: Mutex::new(ask_rx),
            ask_responder: Arc::new(Mutex::new(None)),
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

/// Build an `AskUserFn` that bridges Rust ↔ JS:
/// 1. Serializes the `AskUserRequest` as a JSON event and sends it via `ask_event_tx`
/// 2. Stores a oneshot sender in `ask_responder`
/// 3. Blocks until JS calls `respond_ask_user()` which sends the answer back
fn build_ask_fn(
    ask_event_tx: tokio_mpsc::UnboundedSender<String>,
    ask_responder: AskResponder,
) -> AskUserFn {
    Arc::new(move |request: AskUserRequest| {
        let tx = ask_event_tx.clone();
        let responder = ask_responder.clone();
        (async move {
            // Serialize the request as a synthetic event JSON
            let questions_value = match serde_json::to_value(&request.questions) {
                Ok(v) => v,
                Err(e) => return Err(format!("serialize ask_user questions: {e}")),
            };
            let event_json = serde_json::json!({
                "kind": "ask_user",
                "payload": { "questions": questions_value }
            });
            let json_str = match serde_json::to_string(&event_json) {
                Ok(s) => s,
                Err(e) => return Err(format!("serialize ask_user event: {e}")),
            };

            // Create a oneshot channel for the response
            let (resp_tx, resp_rx) =
                oneshot::channel::<std::result::Result<AskUserResponse, String>>();

            // Store the sender so respond_ask_user() can find it
            {
                let mut guard = responder.lock().await;
                *guard = Some(resp_tx);
            }

            // Send the event to the JS side (will be picked up by next())
            if let Err(e) = tx.send(json_str) {
                return Err(format!("send ask_user event: {e}"));
            }

            // Block until JS responds
            match resp_rx.await {
                Ok(result) => result,
                Err(_) => Err("ask_user response channel closed".into()),
            }
        })
        .boxed()
    })
}

/// Version string for the native addon.
#[napi]
pub fn version() -> String {
    env!("EVOT_VERSION").to_string()
}

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

#[napi]
pub async fn start_server_background(
    port: Option<u16>,
    model: Option<String>,
) -> Result<Option<String>> {
    let mut config = evot::conf::Config::load()
        .map_err(|e| Error::from_reason(format!("config load failed: {e}")))?
        .with_model(model);
    if let Some(p) = port {
        config = config.with_port(p);
    }
    let actual_port = config.server.port;
    let host = config.server.host.clone();
    let addr = format!("{host}:{actual_port}");

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(_) => return Ok(None),
    };

    let agent = evot::gateway::service::build_agent(&config)
        .map_err(|e| Error::from_reason(format!("agent init: {e}")))?;

    let cancel = tokio_util::sync::CancellationToken::new();
    let channel_handles =
        evot::gateway::registry::spawn_all(&config.channels, agent.clone(), cancel);

    let mut channels = Vec::new();
    if config.channels.feishu.is_some() {
        channels.push("feishu");
    }

    let server = evot::gateway::channels::http::Server::new(agent);
    tokio::spawn(async move {
        let _ = axum::serve(listener, server.router()).await;
    });

    let info = serde_json::json!({
        "port": actual_port,
        "address": format!("http://{addr}"),
        "channels": channels,
        "channelCount": channel_handles.len(),
    });
    serde_json::to_string(&info)
        .map(Some)
        .map_err(|e| Error::from_reason(format!("serialize: {e}")))
}
