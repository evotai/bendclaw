use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use evot::agent::Agent;
use evot::agent::BackgroundReason;
use evot::agent::ForkRequest;
use evot::agent::HostTools;
use evot::agent::QueryRequest;
use evot::agent::ToolMode;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::compaction::NapiCompaction;
use crate::convert::parse_content_blocks;
use crate::fork::NapiForkedAgent;
use crate::host::parse_host_tool_specs;
use crate::host::HostResponders;
use crate::host::NapiHostBridge;
use crate::run::NapiRun;
use crate::run::NapiSubmitOutcome;
use crate::tracing::init_tracing;

#[napi]
pub struct NapiAgent {
    agent: Arc<Agent>,
    env_file_path: String,
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

        let env_file_path = config.env_file_path.to_string_lossy().to_string();
        let agent = evot::gateway::service::build_agent(&config)
            .await
            .map_err(|e| Error::from_reason(format!("agent init: {e}")))?;

        Ok(Self {
            agent,
            env_file_path,
        })
    }

    /// Current model name.
    #[napi(getter)]
    pub fn model(&self) -> String {
        self.agent.llm().model
    }

    /// Set the active model by model spec (e.g. "deepseek-chat" or "openrouter:google/gemini-2.5-pro").
    #[napi(setter)]
    pub fn set_model(&mut self, model: String) -> Result<()> {
        let config = self.load_config()?;
        self.agent
            .set_model_by_spec(&config, &model)
            .map_err(|e| Error::from_reason(format!("model switch failed: {e}")))?;
        Ok(())
    }

    /// Current working directory.
    #[napi(getter)]
    pub fn cwd(&self) -> String {
        self.agent.cwd().to_string()
    }

    /// Send a prompt and get a stream of events.
    ///
    /// `host_specs_json` is a JSON array of host tool specs the TS side
    /// registered (ask_user, plan, extension tools). When present and the mode
    /// permits, these are attached to the run and their execution is delegated
    /// back to JS via `host_tool_call` events.
    #[napi]
    pub async fn query(
        &self,
        prompt: String,
        session_id: Option<String>,
        tool_mode: Option<String>,
        content_json: Option<String>,
        host_specs_json: Option<String>,
    ) -> Result<NapiSubmitOutcome> {
        let (host_event_tx, host_event_rx) = tokio_mpsc::unbounded_channel::<String>();
        let host_responders: HostResponders =
            Arc::new(Mutex::new(std::collections::HashMap::new()));

        let mode = match tool_mode.as_deref() {
            Some("interactive") | Some("planning_interactive") => {
                if tool_mode.as_deref() == Some("planning_interactive") {
                    ToolMode::Planning
                } else {
                    ToolMode::Interactive
                }
            }
            Some("planning") => ToolMode::Planning,
            Some("readonly") => ToolMode::Readonly,
            _ => ToolMode::Headless,
        };

        // Assemble host tools from the registered specs. Readonly runs carry no
        // host bridge (matches ToolMode::allows_host_tools).
        let host_tools = if mode.allows_host_tools() {
            let specs =
                parse_host_tool_specs(host_specs_json.as_deref()).map_err(Error::from_reason)?;
            if specs.is_empty() {
                None
            } else {
                let bridge = Arc::new(NapiHostBridge::new(host_event_tx, host_responders.clone()));
                Some(HostTools::new(bridge, specs))
            }
        } else {
            None
        };

        let request = if let Some(json) = content_json {
            let input = parse_content_blocks(&json).map_err(Error::from_reason)?;

            if input.is_empty() {
                return Err(Error::from_reason("empty content"));
            }

            QueryRequest::with_input(input)
                .session_id(session_id)
                .mode(mode)
                .host_tools(host_tools)
                .source("repl")
        } else {
            QueryRequest::text(prompt)
                .session_id(session_id)
                .mode(mode)
                .host_tools(host_tools)
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
                        host_event_rx: Mutex::new(Some(host_event_rx)),
                        host_responders,
                    })),
                    message: None,
                })
            }
        }
    }

    #[napi]
    pub async fn create_session(&self) -> Result<String> {
        let meta = self
            .agent
            .create_session("repl")
            .await
            .map_err(|e| Error::from_reason(format!("create session: {e}")))?;

        serde_json::to_string(&meta).map_err(|e| Error::from_reason(format!("serialize: {e}")))
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

    #[napi]
    pub async fn delete_session(&self, session_id: String) -> Result<bool> {
        self.agent
            .delete_session(&session_id)
            .await
            .map_err(|e| Error::from_reason(format!("delete session: {e}")))
    }

    #[napi]
    pub fn background_processes(&self, session_id: String) -> Result<String> {
        serialize_process_summaries(self.agent.background_processes(&session_id))
    }

    #[napi]
    pub async fn stop_background_process(
        &self,
        session_id: String,
        task_id: String,
    ) -> Result<Option<String>> {
        let summary = self
            .agent
            .stop_background_process(&session_id, &task_id)
            .await
            .map_err(|e| Error::from_reason(format!("stop background process: {e}")))?;
        summary.map(serialize_process_summary).transpose()
    }

    /// Detach every foreground shell in a session so the turn can be reclaimed.
    ///
    /// The processes keep running; only the waiting ends. Returns how many moved.
    #[napi]
    pub fn background_foreground_processes(&self, session_id: String) -> u32 {
        self.agent
            .background_foreground_processes(&session_id, BackgroundReason::UserRequested)
            as u32
    }

    /// Same detach, attributed to a queued message needing delivery.
    ///
    /// Steering is only inspected between tool calls, so a foreground shell
    /// holds a typed message until it finishes. Detaching lets the message land
    /// while the command keeps running.
    #[napi]
    pub fn background_foreground_processes_for_message(&self, session_id: String) -> u32 {
        self.agent
            .background_foreground_processes(&session_id, BackgroundReason::MessageDelivery)
            as u32
    }

    /// Blocking `task_output` waits in flight for this session.
    ///
    /// Such a wait holds the turn while the task it watches is already
    /// backgrounded, so no foreground shell exists to detach.
    #[napi]
    pub fn blocking_task_waits(&self, session_id: String) -> u32 {
        self.agent.blocking_task_waits(&session_id) as u32
    }

    /// End in-flight blocking waits, returning how many were released. The
    /// watched tasks keep running; only the waiting ends.
    #[napi]
    pub fn release_blocking_task_waits(&self, session_id: String) -> u32 {
        self.agent.release_blocking_task_waits(&session_id) as u32
    }

    /// Completion notices queued for this session but not yet delivered.
    ///
    /// Does not consume them: only a turn can carry them, so the UI polls this
    /// to decide whether to open one.
    #[napi]
    pub fn pending_process_notifications(&self, session_id: String) -> u32 {
        self.agent.pending_process_notifications(&session_id) as u32
    }

    #[napi]
    pub async fn stop_all_background_processes(&self, session_id: String) -> Result<String> {
        serialize_process_summaries(self.agent.stop_all_background_processes(&session_id).await)
    }

    /// Kill every background process across all sessions without awaiting.
    /// Safe to call immediately before `fastExit`, which skips async teardown.
    #[napi]
    pub fn kill_all_background_processes_now(&self) -> u32 {
        self.agent.kill_all_background_processes_now() as u32
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

    /// Load the effective context transcript for a session, after control markers.
    #[napi]
    pub async fn load_context_transcript(&self, session_id: String) -> Result<String> {
        let items = self
            .agent
            .load_context_transcript(&session_id)
            .await
            .map_err(|e| Error::from_reason(format!("load context transcript: {e}")))?;

        serde_json::to_string(&items).map_err(|e| Error::from_reason(format!("serialize: {e}")))
    }

    /// Load a bounded resume view without superseded history or Engine-only
    /// compact snapshot fields.
    #[napi]
    pub async fn load_resume_transcript(&self, session_id: String) -> Result<String> {
        let items = self
            .agent
            .load_resume_transcript(&session_id)
            .await
            .map_err(|e| Error::from_reason(format!("load resume transcript: {e}")))?;

        serde_json::to_string(&items).map_err(|e| Error::from_reason(format!("serialize: {e}")))
    }

    /// Find a single session by ID.
    #[napi]
    pub async fn find_session(&self, session_id: String) -> Result<Option<String>> {
        let meta = self
            .agent
            .find_session(&session_id)
            .await
            .map_err(|e| Error::from_reason(format!("find session: {e}")))?;
        match meta {
            Some(m) => serde_json::to_string(&m)
                .map(Some)
                .map_err(|e| Error::from_reason(format!("serialize: {e}"))),
            None => Ok(None),
        }
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

    /// List agent variables as JSON array of {key, value, updated_at}.
    #[napi]
    pub fn list_variables(&self) -> Result<String> {
        match self.agent.variables() {
            Some(vars) => {
                let items: Vec<_> = vars
                    .list_global()
                    .iter()
                    .map(|v| {
                        serde_json::json!({
                            "key": v.key,
                            "value": v.value,
                            "updated_at": v.updated_at,
                        })
                    })
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

    /// Get config info: active provider/protocol, env path, base URL, and configured models.
    /// Reloads the config file so edits are visible without restarting the CLI.
    #[napi]
    pub fn config_info(&self) -> Result<String> {
        let config = self.load_config()?;
        let llm = self.agent.llm();
        let provider = llm.provider.clone();
        let env_path = config.env_file_path.to_string_lossy().to_string();
        let has_api_key = !llm.api_key.is_empty();
        let available = self.collect_models(&config);
        let thinking_level = display_thinking_level(&llm);
        let info = serde_json::json!({
            "provider": provider,
            "protocol": llm.protocol.to_string(),
            "envPath": env_path,
            "hasApiKey": has_api_key,
            "baseUrl": llm.base_url,
            "availableModels": available,
            "thinkingLevel": thinking_level,
        });
        serde_json::to_string(&info).map_err(|e| Error::from_reason(format!("serialize: {e}")))
    }

    /// Get configured models as provider-qualified specs. Provider qualification
    /// keeps entries distinct when multiple providers expose the same model id;
    /// config info also carries each provider's wire protocol.
    #[napi]
    pub fn available_models(&self) -> Result<Vec<String>> {
        let config = self.load_config()?;
        Ok(self
            .collect_models(&config)
            .into_iter()
            .filter_map(|entry| entry.get("spec")?.as_str().map(str::to_string))
            .collect())
    }

    fn load_config(&self) -> Result<evot::conf::Config> {
        evot::conf::Config::load_with_env_file(Some(&self.env_file_path))
            .map_err(|e| Error::from_reason(format!("config reload failed: {e}")))
    }

    fn collect_models(&self, config: &evot::conf::Config) -> Vec<serde_json::Value> {
        let llm = self.agent.llm();
        let free_meta = crate::cloud_model_meta();
        // The server names and orders its own groups; membership decides which
        // providers are cloud, so no name is hardcoded here.
        let cloud_groups = crate::cloud_provider_groups();
        let mut models = Vec::new();
        for (provider, profile) in &config.providers {
            for model in &profile.models {
                let model = model.trim();
                if !model.is_empty() {
                    let mut entry = serde_json::json!({
                        "provider": provider,
                        "protocol": profile.protocol.to_string(),
                        "model": model,
                        "spec": format!("{provider}:{model}"),
                    });
                    if let Some((label, order)) = cloud_groups.get(provider) {
                        entry["group_label"] = serde_json::json!(label);
                        entry["group_order"] = serde_json::json!(order);
                        // Catalog rank inside the tier: higher shows earlier
                        // when the picker merges protocol providers.
                        entry["sort_order"] = serde_json::json!(config
                            .cloud_model_sorts
                            .get(model)
                            .copied()
                            .unwrap_or(0));
                        if let Some(meta) = free_meta.get(model) {
                            entry["free"] = serde_json::json!({
                                "display_name": meta.display_name,
                                "tagline": meta.tagline,
                                "is_new": meta.is_new,
                                "tier": meta.tier,
                            });
                        } else {
                            entry["free"] = serde_json::json!({});
                        }
                    }
                    models.push(entry);
                }
            }
        }
        let current_is_listed = config.providers.get(&llm.provider).is_some_and(|profile| {
            profile
                .models
                .iter()
                .any(|model| model.trim() == llm.model.trim())
        });
        if !llm.model.trim().is_empty()
            && !current_is_listed
            && config.providers.contains_key(&llm.provider)
        {
            models.push(serde_json::json!({
                "provider": llm.provider,
                "protocol": llm.protocol.to_string(),
                "model": llm.model,
                "spec": format!("{}:{}", llm.provider, llm.model),
            }));
        }
        models
    }

    /// Switch the active provider by model spec.
    #[napi]
    pub fn set_provider(&self, provider: String) -> Result<()> {
        let config = self.load_config()?;
        self.agent
            .set_provider_by_spec(&config, &provider)
            .map_err(|e| Error::from_reason(format!("invalid provider: {e}")))
    }

    /// Re-resolve the live model selection after login, logout, or key
    /// recovery. Returns false only when nothing is configured any more, i.e.
    /// the one case that needs `/login`.
    #[napi]
    pub fn reload_selection(&self) -> Result<bool> {
        let config = self.load_config()?;
        Ok(self.agent.reload_selection(&config).has_model())
    }

    /// Reload provider/model selection from disk for session resume. Unlike an
    /// interactive switch, this reapplies the current configured thinking level.
    /// Returns false when the saved selection is unavailable and the current
    /// live selection was refreshed instead.
    #[napi]
    pub fn reload_provider(&self, provider: String) -> Result<bool> {
        let config = self.load_config()?;
        self.agent
            .reload_provider_for_resume(&config, &provider)
            .map_err(|e| Error::from_reason(format!("invalid provider: {e}")))
    }

    /// Advance the thinking level to the next tier the current model supports,
    /// wrapping around, and persist it as the default for future sessions
    /// (pi-style double write: the session records its own level per run).
    /// Returns the new level's display label, or `null` when the model has no
    /// selectable reasoning levels.
    #[napi]
    pub fn cycle_thinking_level(&self) -> Option<String> {
        let level = self.agent.cycle_thinking_level()?;
        // Best-effort: the live session keeps the new level even when the
        // config write fails; the default then falls back on next start.
        if let Ok(mut config) = self.load_config() {
            let provider = self.agent.llm().provider.clone();
            let _ = evot::conf::persist_default_thinking_level(&mut config, &provider, level);
        }
        Some(display_thinking_level(&self.agent.llm()))
    }

    /// Apply a named thinking level when supported by the active model.
    /// Used on session resume so the session's recorded effort wins over the
    /// config default applied by `reload_provider`.
    #[napi]
    pub fn restore_thinking_level(&self, level: String) {
        self.agent.restore_thinking_level(&level);
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

    #[napi]
    pub fn add_skills_dirs(&self, dirs: Vec<String>) {
        let paths: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
        self.agent.add_skills_dirs(paths);
    }

    #[napi]
    pub fn set_skill_names(&self, names: Vec<String>) -> Result<()> {
        self.agent
            .set_skill_names(names)
            .map_err(|error| Error::from_reason(error.to_string()))
    }

    /// The fully-resolved, ordered skills directories the agent scans (managed
    /// builtins + global + config/env-file EVOT_SKILLS_DIRS + claude).
    /// The CLI reads this so `/skill list` and the banner match what the agent
    /// actually loads, instead of re-deriving from process.env alone.
    #[napi]
    pub fn skills_dirs(&self) -> Vec<String> {
        self.agent
            .skills_dirs()
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    }

    /// Start an abortable manual session compaction.
    #[napi]
    pub fn compact(
        &self,
        session_id: String,
        custom_instructions: Option<String>,
    ) -> NapiCompaction {
        NapiCompaction::start(self.agent.clone(), session_id, custom_instructions)
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

fn process_summary_json(summary: &evot_engine::tools::ProcessSummary) -> serde_json::Value {
    serde_json::json!({
        "task_id": summary.task_id,
        "command": summary.command,
        "cwd": summary.cwd,
        "output_path": summary.output_path,
        "status": summary.status.as_str(),
        "exit_code": summary.exit_code,
        "elapsed_ms": summary.elapsed.as_millis(),
        "output_file_truncated": summary.output_file_truncated,
        "stopped_by_user": summary.stopped_by_user,
    })
}

fn serialize_process_summary(summary: evot_engine::tools::ProcessSummary) -> Result<String> {
    serde_json::to_string(&process_summary_json(&summary))
        .map_err(|e| Error::from_reason(format!("serialize background process: {e}")))
}

fn serialize_process_summaries(
    summaries: Vec<evot_engine::tools::ProcessSummary>,
) -> Result<String> {
    let values = summaries
        .iter()
        .map(process_summary_json)
        .collect::<Vec<_>>();
    serde_json::to_string(&values)
        .map_err(|e| Error::from_reason(format!("serialize background processes: {e}")))
}

/// Footer label for the active reasoning effort, mirroring pi's footer:
/// the abstract level name (`off`/`low`/`medium`/`high`/`xhigh`/`max`) is shown
/// verbatim — it is never translated through the model's
/// `thinking_level_map`.
///
/// Returns an empty string when the model honors no selectable reasoning
/// effort (e.g. an OpenAI-compatible provider without the reasoning-effort
/// capability), which tells the footer to omit the indicator — the same gate
/// pi applies via `model.reasoning`.
fn display_thinking_level(llm: &evot::conf::LlmConfig) -> String {
    let model_config = &llm.model_config;
    if !model_config.reasoning() || model_config.supported_thinking_levels().is_empty() {
        return String::new();
    }
    llm.thinking_level.as_str().to_string()
}
