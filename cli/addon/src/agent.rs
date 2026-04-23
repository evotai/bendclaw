use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use evot::agent::Agent;
use evot::agent::ForkRequest;
use evot::agent::QueryRequest;
use evot::agent::ToolMode;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::ask::build_ask_fn;
use crate::ask::AskResponder;
use crate::convert::parse_content_blocks;
use crate::fork::NapiForkedAgent;
use crate::run::NapiRun;
use crate::run::NapiSubmitOutcome;
use crate::tracing::init_tracing;

#[napi]
pub struct NapiAgent {
    agent: Arc<Agent>,
    config: evot::conf::Config,
}

#[napi]
impl NapiAgent {
    /// Load config from disk and create an agent.
    #[napi(factory)]
    pub async fn create(model: Option<String>, env_file: Option<String>) -> Result<Self> {
        init_tracing();

        let config = evot::conf::Config::load_with_env_file(env_file.as_deref())
            .map_err(|e| Error::from_reason(format!("config load failed: {e}")))?
            .with_model(model)
            .map_err(|e| Error::from_reason(format!("config model: {e}")))?;
        config
            .validate()
            .map_err(|e| Error::from_reason(format!("config validation: {e}")))?;

        let agent = evot::gateway::service::build_agent(&config)
            .await
            .map_err(|e| Error::from_reason(format!("agent init: {e}")))?;

        Ok(Self { agent, config })
    }

    /// Current model name.
    #[napi(getter)]
    pub fn model(&self) -> String {
        self.agent.llm().model
    }

    /// Set the active model by model spec (e.g. "deepseek-chat" or "openrouter:google/gemini-2.5-pro").
    #[napi(setter)]
    pub fn set_model(&mut self, model: String) {
        self.agent.set_model_by_spec(&self.config, &model);
    }

    /// Current working directory.
    #[napi(getter)]
    pub fn cwd(&self) -> String {
        self.agent.cwd().to_string()
    }

    /// Send a prompt and get a stream of events.
    #[napi]
    pub async fn query(
        &self,
        prompt: String,
        session_id: Option<String>,
        tool_mode: Option<String>,
        content_json: Option<String>,
    ) -> Result<NapiSubmitOutcome> {
        let (ask_event_tx, ask_event_rx) = tokio_mpsc::unbounded_channel::<String>();
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
            let input = parse_content_blocks(&json).map_err(Error::from_reason)?;

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

        let outcome = self
            .agent
            .submit(request)
            .await
            .map_err(|e| Error::from_reason(format!("query failed: {e}")))?;

        match outcome {
            evot::agent::SubmitOutcome::Command(msg) => Ok(NapiSubmitOutcome {
                kind: "command".into(),
                run: std::sync::Mutex::new(None),
                message: Some(msg),
            }),
            evot::agent::SubmitOutcome::Run(run) => {
                let sid = run.session_id.clone();
                let handle = run.handle();

                Ok(NapiSubmitOutcome {
                    kind: "run".into(),
                    run: std::sync::Mutex::new(Some(NapiRun {
                        inner: Mutex::new(run),
                        handle,
                        cached_session_id: sid,
                        aborted: Arc::new(AtomicBool::new(false)),
                        abort_notify: Arc::new(Notify::new()),
                        ask_event_rx: Mutex::new(Some(ask_event_rx)),
                        ask_responder,
                    })),
                    message: None,
                })
            }
        }
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

    #[napi]
    pub async fn list_sessions_with_text(&self, limit: Option<u32>) -> Result<String> {
        let items = self
            .agent
            .list_sessions_with_text(limit.unwrap_or(0) as usize)
            .await
            .map_err(|e| Error::from_reason(format!("list sessions with text: {e}")))?;
        serde_json::to_string(&items).map_err(|e| Error::from_reason(format!("serialize: {e}")))
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
        Ok(NapiForkedAgent::new(forked))
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
        let provider = llm.provider.clone();
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
        for (_, profile) in &self.config.providers {
            let trimmed = profile.model.trim();
            if !trimmed.is_empty() && !models.contains(&trimmed.to_string()) {
                models.push(trimmed.to_string());
            }
        }
        let trimmed = llm.model.trim();
        if !trimmed.is_empty() && !models.contains(&trimmed.to_string()) {
            models.push(trimmed.to_string());
        }
        models
    }

    /// Switch the active provider by model spec.
    #[napi]
    pub fn set_provider(&self, provider: String) -> Result<()> {
        self.agent
            .set_provider_by_spec(&self.config, &provider)
            .map_err(|e| Error::from_reason(format!("invalid provider: {e}")))
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

    /// Send a steering message into a running session.
    #[napi]
    pub fn steer(&self, session_id: String, text: String, content_json: Option<String>) {
        let input = if let Some(json) = content_json {
            if let Ok(blocks) = parse_content_blocks(&json) {
                if blocks.is_empty() {
                    vec![evot_engine::Content::Text { text }]
                } else {
                    blocks
                }
            } else {
                vec![evot_engine::Content::Text { text }]
            }
        } else {
            vec![evot_engine::Content::Text { text }]
        };
        self.agent.steer(&session_id, input);
    }

    /// Send a follow-up message to a running session.
    #[napi]
    pub fn follow_up(&self, session_id: String, text: String) {
        self.agent.follow_up(&session_id, &text);
    }

    /// Abort a running session.
    #[napi]
    pub fn abort_run(&self, session_id: String) {
        self.agent.abort_run(&session_id);
    }
}
