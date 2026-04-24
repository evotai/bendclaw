//! Stateful Agent struct — wraps the agent loop with state management,
//! steering/follow-up queues, and abort support.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::handle::QueueMode;
use super::handle::RunHandle;
use crate::context::CompactionStrategy;
use crate::context::ContextConfig;
use crate::context::ExecutionLimits;
use crate::provider::ModelConfig;
use crate::provider::StreamProvider;
use crate::r#loop::AfterTurnFn;
use crate::r#loop::BeforeTurnFn;
use crate::tools::guard::PathGuard;
use crate::types::*;

/// The main Agent. Owns state, tools, and provider.
pub struct Agent {
    // State
    pub system_prompt: String,
    pub model: String,
    pub api_key: String,
    pub thinking_level: ThinkingLevel,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub(super) model_config: Option<ModelConfig>,
    pub(super) messages: Vec<AgentMessage>,
    pub(super) tools: Vec<Box<dyn AgentTool>>,
    pub(super) provider: Arc<dyn StreamProvider>,

    // Sandbox
    pub(super) cwd: PathBuf,
    pub(super) path_guard: Arc<PathGuard>,

    // Queues (shared with the loop via Arc<Mutex>)
    pub(super) steering_queue: Arc<Mutex<Vec<AgentMessage>>>,
    pub(super) follow_up_queue: Arc<Mutex<Vec<AgentMessage>>>,
    pub(super) steering_mode: QueueMode,
    pub(super) follow_up_mode: QueueMode,

    // Context, limits & caching
    pub context_config: Option<ContextConfig>,
    pub(super) context_management_disabled: bool,
    pub execution_limits: Option<ExecutionLimits>,
    pub cache_config: CacheConfig,
    pub tool_execution: ToolExecutionStrategy,
    pub retry_policy: crate::retry::RetryPolicy,

    // Lifecycle callbacks
    pub(super) before_turn: Option<BeforeTurnFn>,
    pub(super) after_turn: Option<AfterTurnFn>,

    // Input filters
    pub(super) input_filters: Vec<Arc<dyn InputFilter>>,

    // Custom compaction strategy
    pub(super) compaction_strategy: Option<Arc<dyn CompactionStrategy>>,

    // Control
    pub(super) cancel: Option<CancellationToken>,
    pub(super) is_streaming: bool,

    // Last run handle (for convenience methods on Agent)
    pub(super) last_run_handle: Option<RunHandle>,

    // Pending completion from a spawned agent loop
    #[allow(clippy::type_complexity)]
    pub(super) pending_completion: Option<JoinHandle<(Vec<Box<dyn AgentTool>>, Vec<AgentMessage>)>>,
}

impl Agent {
    pub fn new(provider: impl StreamProvider + 'static) -> Self {
        Self {
            system_prompt: String::new(),
            model: String::new(),
            api_key: String::new(),
            thinking_level: ThinkingLevel::default(),
            max_tokens: None,
            temperature: None,
            model_config: None,
            messages: Vec::new(),
            tools: Vec::new(),
            provider: Arc::new(provider),
            cwd: PathBuf::new(),
            path_guard: Arc::new(PathGuard::open()),
            steering_queue: Arc::new(Mutex::new(Vec::new())),
            follow_up_queue: Arc::new(Mutex::new(Vec::new())),
            steering_mode: QueueMode::OneAtATime,
            follow_up_mode: QueueMode::OneAtATime,
            context_config: None,
            context_management_disabled: false,
            execution_limits: Some(ExecutionLimits::default()),
            cache_config: CacheConfig::default(),
            tool_execution: ToolExecutionStrategy::default(),
            retry_policy: crate::retry::RetryPolicy::default(),
            before_turn: None,
            after_turn: None,
            input_filters: Vec::new(),
            compaction_strategy: None,
            cancel: None,
            is_streaming: false,
            last_run_handle: None,
            pending_completion: None,
        }
    }

    // -- Builder-style setters --

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = key.into();
        self
    }

    pub fn with_thinking(mut self, level: ThinkingLevel) -> Self {
        self.thinking_level = level;
        self
    }

    pub fn with_tools(mut self, tools: Vec<Box<dyn AgentTool>>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    pub fn with_path_guard(mut self, guard: Arc<PathGuard>) -> Self {
        self.path_guard = guard;
        self
    }

    pub fn with_model_config(mut self, config: ModelConfig) -> Self {
        self.model_config = Some(config);
        self
    }

    pub fn with_max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    pub fn with_context_config(mut self, config: ContextConfig) -> Self {
        self.context_config = Some(config);
        self
    }

    pub fn with_cache_config(mut self, config: CacheConfig) -> Self {
        self.cache_config = config;
        self
    }

    pub fn with_tool_execution(mut self, strategy: ToolExecutionStrategy) -> Self {
        self.tool_execution = strategy;
        self
    }

    pub fn with_retry_policy(mut self, policy: crate::retry::RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub fn with_retry_disabled(mut self) -> Self {
        self.retry_policy = crate::retry::RetryPolicy::disabled();
        self
    }

    pub fn with_max_retries(mut self, n: usize) -> Self {
        self.retry_policy = crate::retry::RetryPolicy::new(n);
        self
    }

    /// Load skills and register the skill tool.
    ///
    /// Appends the skills index to the system prompt (XML per the
    /// [AgentSkills standard](https://agentskills.io)) and registers a
    /// `SkillTool` so the LLM can activate skills by name.
    ///
    /// **Must be called after `with_tools()`** — `with_tools()` replaces the
    /// tool list, so calling it afterwards would remove the SkillTool.
    pub fn with_skills(mut self, skills: crate::tools::skill::SkillSet) -> Self {
        if skills.is_empty() {
            return self;
        }
        let prompt_fragment = skills.format_for_prompt();
        if self.system_prompt.is_empty() {
            self.system_prompt = prompt_fragment;
        } else {
            self.system_prompt = format!("{}\n\n{}", self.system_prompt, prompt_fragment);
        }
        self.tools
            .push(Box::new(crate::tools::skill::SkillTool::new(
                std::sync::Arc::new(skills),
            )));
        self
    }

    pub fn with_execution_limits(mut self, limits: ExecutionLimits) -> Self {
        self.execution_limits = Some(limits);
        self
    }

    pub fn with_messages(mut self, msgs: Vec<AgentMessage>) -> Self {
        self.messages = msgs;
        self
    }

    pub fn on_before_turn(
        mut self,
        f: impl Fn(&[AgentMessage], usize) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.before_turn = Some(Arc::new(f));
        self
    }

    pub fn on_after_turn(
        mut self,
        f: impl Fn(&[AgentMessage], &Usage) + Send + Sync + 'static,
    ) -> Self {
        self.after_turn = Some(Arc::new(f));
        self
    }

    /// Add an input filter. Filters run in order on user messages before the LLM call.
    pub fn with_input_filter(mut self, filter: impl InputFilter + 'static) -> Self {
        self.input_filters.push(Arc::new(filter));
        self
    }

    /// Set a custom compaction strategy. When set, replaces the default
    /// `compact_messages()` call during context compaction.
    pub fn with_compaction_strategy(mut self, strategy: impl CompactionStrategy + 'static) -> Self {
        self.compaction_strategy = Some(Arc::new(strategy));
        self
    }

    /// Disable automatic context compaction and execution limits.
    /// This takes precedence over auto-derivation from `ModelConfig.context_window`.
    pub fn without_context_management(mut self) -> Self {
        self.context_config = None;
        self.context_management_disabled = true;
        self.execution_limits = None;
        self
    }

    // -- State access --

    pub fn messages(&self) -> &[AgentMessage] {
        &self.messages
    }

    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    pub fn set_tools(&mut self, tools: Vec<Box<dyn AgentTool>>) {
        self.tools = tools;
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }

    pub fn append_message(&mut self, msg: AgentMessage) {
        self.messages.push(msg);
    }

    pub fn replace_messages(&mut self, msgs: Vec<AgentMessage>) {
        self.messages = msgs;
    }

    pub fn save_messages(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.messages)
    }

    pub fn restore_messages(&mut self, json: &str) -> Result<(), serde_json::Error> {
        let msgs: Vec<AgentMessage> = serde_json::from_str(json)?;
        self.messages = msgs;
        Ok(())
    }

    // -- Queue management --

    /// Queue a steering message (delegates to last run handle).
    pub fn steer(&self, msg: AgentMessage) {
        if let Some(ref h) = self.last_run_handle {
            h.steer(msg);
        } else {
            self.steering_queue.lock().push(msg);
        }
    }

    /// Queue a follow-up message (delegates to last run handle).
    pub fn follow_up(&self, msg: AgentMessage) {
        if let Some(ref h) = self.last_run_handle {
            h.follow_up(msg);
        } else {
            self.follow_up_queue.lock().push(msg);
        }
    }

    pub fn clear_steering_queue(&self) {
        self.steering_queue.lock().clear();
        if let Some(ref h) = self.last_run_handle {
            h.clear_steering();
        }
    }

    pub fn clear_follow_up_queue(&self) {
        self.follow_up_queue.lock().clear();
        if let Some(ref h) = self.last_run_handle {
            h.clear_follow_up();
        }
    }

    pub fn clear_all_queues(&self) {
        self.clear_steering_queue();
        self.clear_follow_up_queue();
    }

    pub fn set_steering_mode(&mut self, mode: QueueMode) {
        self.steering_mode = mode;
    }

    pub fn set_follow_up_mode(&mut self, mode: QueueMode) {
        self.follow_up_mode = mode;
    }

    /// Get the last run handle (if any).
    pub fn run_handle(&self) -> Option<&RunHandle> {
        self.last_run_handle.as_ref()
    }

    // -- Control --

    pub fn abort(&self) {
        if let Some(ref h) = self.last_run_handle {
            h.abort();
        } else if let Some(ref cancel) = self.cancel {
            cancel.cancel();
        }
    }

    pub async fn reset(&mut self) {
        // Cancel cooperatively first, then await to recover tools
        if let Some(ref h) = self.last_run_handle {
            h.abort();
        } else if let Some(ref cancel) = self.cancel {
            cancel.cancel();
        }
        if let Some(handle) = self.pending_completion.take() {
            // Await the cancelled task to recover tools; ignore panic
            if let Ok((tools, _messages)) = handle.await {
                self.tools = tools;
            }
        }
        self.messages.clear();
        self.clear_all_queues();
        self.is_streaming = false;
        self.cancel = None;
        self.last_run_handle = None;
    }
}
