//! Stateful Agent struct — wraps the agent loop with state management,
//! steering/follow-up queues, and abort support.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agent_loop::agent_loop;
use crate::agent_loop::agent_loop_continue;
use crate::agent_loop::AfterTurnFn;
use crate::agent_loop::AgentLoopConfig;
use crate::agent_loop::BeforeTurnFn;
use crate::agent_loop::OnErrorFn;
use crate::context::CompactionStrategy;
use crate::context::ContextConfig;
use crate::context::ExecutionLimits;
use crate::mcp::McpClient;
use crate::mcp::McpError;
use crate::mcp::McpToolAdapter;
use crate::provider::ModelConfig;
use crate::provider::StreamProvider;
use crate::types::*;

/// Queue mode for steering and follow-up messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMode {
    /// Deliver one message per turn
    OneAtATime,
    /// Deliver all queued messages at once
    All,
}

/// The main Agent. Owns state, tools, and provider.
pub struct Agent {
    // State
    pub system_prompt: String,
    pub model: String,
    pub api_key: String,
    pub thinking_level: ThinkingLevel,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    model_config: Option<ModelConfig>,
    messages: Vec<AgentMessage>,
    tools: Vec<Box<dyn AgentTool>>,
    provider: Arc<dyn StreamProvider>,

    // Queues (shared with the loop via Arc<Mutex>)
    steering_queue: Arc<Mutex<Vec<AgentMessage>>>,
    follow_up_queue: Arc<Mutex<Vec<AgentMessage>>>,
    steering_mode: QueueMode,
    follow_up_mode: QueueMode,

    // Context, limits & caching
    pub context_config: Option<ContextConfig>,
    context_management_disabled: bool,
    pub execution_limits: Option<ExecutionLimits>,
    pub cache_config: CacheConfig,
    pub tool_execution: ToolExecutionStrategy,
    pub retry_config: crate::retry::RetryConfig,

    // Lifecycle callbacks
    before_turn: Option<BeforeTurnFn>,
    after_turn: Option<AfterTurnFn>,
    on_error: Option<OnErrorFn>,

    // Input filters
    input_filters: Vec<Arc<dyn InputFilter>>,

    // Custom compaction strategy
    compaction_strategy: Option<Arc<dyn CompactionStrategy>>,

    // Control
    cancel: Option<CancellationToken>,
    is_streaming: bool,

    // Pending completion from a spawned agent loop
    #[allow(clippy::type_complexity)]
    pending_completion: Option<JoinHandle<(Vec<Box<dyn AgentTool>>, Vec<AgentMessage>)>>,
}

impl Agent {
    pub fn new(provider: impl StreamProvider + 'static) -> Self {
        Self {
            system_prompt: String::new(),
            model: String::new(),
            api_key: String::new(),
            thinking_level: ThinkingLevel::Off,
            max_tokens: None,
            temperature: None,
            model_config: None,
            messages: Vec::new(),
            tools: Vec::new(),
            provider: Arc::new(provider),
            steering_queue: Arc::new(Mutex::new(Vec::new())),
            follow_up_queue: Arc::new(Mutex::new(Vec::new())),
            steering_mode: QueueMode::OneAtATime,
            follow_up_mode: QueueMode::OneAtATime,
            context_config: None,
            context_management_disabled: false,
            execution_limits: Some(ExecutionLimits::default()),
            cache_config: CacheConfig::default(),
            tool_execution: ToolExecutionStrategy::default(),
            retry_config: crate::retry::RetryConfig::default(),
            before_turn: None,
            after_turn: None,
            on_error: None,
            input_filters: Vec::new(),
            compaction_strategy: None,
            cancel: None,
            is_streaming: false,
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

    pub fn with_retry_config(mut self, config: crate::retry::RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Load skills and append their index to the system prompt.
    ///
    /// The skills index is appended as XML per the [AgentSkills standard](https://agentskills.io).
    /// The agent can then read individual SKILL.md files using the `read_file` tool
    /// when it decides a skill is relevant.
    pub fn with_skills(mut self, skills: crate::skills::SkillSet) -> Self {
        let prompt_fragment = skills.format_for_prompt();
        if !prompt_fragment.is_empty() {
            if self.system_prompt.is_empty() {
                self.system_prompt = prompt_fragment;
            } else {
                self.system_prompt = format!("{}\n\n{}", self.system_prompt, prompt_fragment);
            }
        }
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

    pub fn on_error(mut self, f: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_error = Some(Arc::new(f));
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

    /// Add a sub-agent tool. The sub-agent runs its own `agent_loop()` when invoked.
    pub fn with_sub_agent(mut self, sub: crate::sub_agent::SubAgentTool) -> Self {
        self.tools.push(Box::new(sub));
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

    // -- OpenAPI integration --

    /// Load tools from an OpenAPI spec file and add them to the agent.
    #[cfg(feature = "openapi")]
    pub async fn with_openapi_file(
        mut self,
        path: impl AsRef<std::path::Path>,
        config: crate::openapi::OpenApiConfig,
        filter: &crate::openapi::OperationFilter,
    ) -> Result<Self, crate::openapi::OpenApiError> {
        let adapters = crate::openapi::OpenApiToolAdapter::from_file(path, config, filter).await?;
        for adapter in adapters {
            self.tools.push(Box::new(adapter));
        }
        Ok(self)
    }

    /// Fetch an OpenAPI spec from a URL and add its tools to the agent.
    #[cfg(feature = "openapi")]
    pub async fn with_openapi_url(
        mut self,
        url: &str,
        config: crate::openapi::OpenApiConfig,
        filter: &crate::openapi::OperationFilter,
    ) -> Result<Self, crate::openapi::OpenApiError> {
        let adapters = crate::openapi::OpenApiToolAdapter::from_url(url, config, filter).await?;
        for adapter in adapters {
            self.tools.push(Box::new(adapter));
        }
        Ok(self)
    }

    /// Parse an OpenAPI spec string and add its tools to the agent.
    #[cfg(feature = "openapi")]
    pub fn with_openapi_spec(
        mut self,
        spec_str: &str,
        config: crate::openapi::OpenApiConfig,
        filter: &crate::openapi::OperationFilter,
    ) -> Result<Self, crate::openapi::OpenApiError> {
        let adapters = crate::openapi::OpenApiToolAdapter::from_str(spec_str, config, filter)?;
        for adapter in adapters {
            self.tools.push(Box::new(adapter));
        }
        Ok(self)
    }

    // -- MCP integration --

    /// Connect to an MCP server via stdio and add its tools to the agent.
    pub async fn with_mcp_server_stdio(
        mut self,
        command: &str,
        args: &[&str],
        env: Option<HashMap<String, String>>,
    ) -> Result<Self, McpError> {
        let client = McpClient::connect_stdio(command, args, env).await?;
        let client = Arc::new(tokio::sync::Mutex::new(client));
        let adapters = McpToolAdapter::from_client(client).await?;
        for adapter in adapters {
            self.tools.push(Box::new(adapter));
        }
        Ok(self)
    }

    /// Connect to an MCP server via HTTP and add its tools to the agent.
    pub async fn with_mcp_server_http(mut self, url: &str) -> Result<Self, McpError> {
        let client = McpClient::connect_http(url).await?;
        let client = Arc::new(tokio::sync::Mutex::new(client));
        let adapters = McpToolAdapter::from_client(client).await?;
        for adapter in adapters {
            self.tools.push(Box::new(adapter));
        }
        Ok(self)
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

    /// Queue a steering message (interrupts agent mid-tool-execution)
    pub fn steer(&self, msg: AgentMessage) {
        self.steering_queue.lock().push(msg);
    }

    /// Queue a follow-up message (processed after agent finishes)
    pub fn follow_up(&self, msg: AgentMessage) {
        self.follow_up_queue.lock().push(msg);
    }

    pub fn clear_steering_queue(&self) {
        self.steering_queue.lock().clear();
    }

    pub fn clear_follow_up_queue(&self) {
        self.follow_up_queue.lock().clear();
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

    // -- Control --

    pub fn abort(&self) {
        if let Some(ref cancel) = self.cancel {
            cancel.cancel();
        }
    }

    pub async fn reset(&mut self) {
        // Cancel cooperatively first, then await to recover tools
        if let Some(ref cancel) = self.cancel {
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
    }

    // -- Prompting --

    /// Send a text prompt. Returns a receiver of AgentEvents immediately,
    /// with the agent loop running concurrently so events stream in real-time.
    ///
    /// Call [`finish()`](Self::finish) after draining the receiver to restore
    /// agent state (messages, tools). `finish()` is also called automatically
    /// at the start of the next `prompt` / `continue_loop` call.
    pub async fn prompt(&mut self, text: impl Into<String>) -> mpsc::UnboundedReceiver<AgentEvent> {
        let msg = AgentMessage::Llm(Message::user(text));
        self.prompt_messages(vec![msg]).await
    }

    /// Send messages as a prompt. Returns a receiver immediately with the
    /// agent loop running concurrently for true streaming.
    ///
    /// Call [`finish()`](Self::finish) after draining events to restore state.
    pub async fn prompt_messages(
        &mut self,
        messages: Vec<AgentMessage>,
    ) -> mpsc::UnboundedReceiver<AgentEvent> {
        self.finish().await; // restore from previous if needed

        assert!(
            !self.is_streaming,
            "Agent is already streaming. Use steer() or follow_up()."
        );

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.is_streaming = true;

        let (tx, rx) = mpsc::unbounded_channel();

        let mut context = AgentContext {
            system_prompt: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: std::mem::take(&mut self.tools),
        };

        let config = self.build_config();

        let handle = tokio::spawn(async move {
            let _new_messages = agent_loop(messages, &mut context, &config, tx, cancel).await;
            (context.tools, context.messages)
        });

        self.pending_completion = Some(handle);
        rx
    }

    /// Send a text prompt, streaming events to a caller-provided sender.
    ///
    /// The caller provides an external sender and sets up a consumer task
    /// before calling this method. This method blocks until the loop finishes
    /// and state is restored — unlike [`prompt()`](Self::prompt) which spawns
    /// the loop concurrently and returns immediately.
    ///
    /// ```rust,no_run
    /// # use bendengine::Agent;
    /// # use bendengine::provider::MockProvider;
    /// # async fn example() {
    /// let mut agent = Agent::new(MockProvider::text("hi"))
    ///     .with_model("mock")
    ///     .with_api_key("test");
    /// let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    /// tokio::spawn(async move {
    ///     while let Some(event) = rx.recv().await { /* real-time */ }
    /// });
    /// agent.prompt_with_sender("hello", tx).await;
    /// # }
    /// ```
    pub async fn prompt_with_sender(
        &mut self,
        text: impl Into<String>,
        tx: mpsc::UnboundedSender<AgentEvent>,
    ) {
        let msg = AgentMessage::Llm(Message::user(text));
        self.prompt_messages_with_sender(vec![msg], tx).await;
    }

    /// Send messages as a prompt, streaming events to a caller-provided sender.
    /// Blocks until the loop finishes and state is restored.
    pub async fn prompt_messages_with_sender(
        &mut self,
        messages: Vec<AgentMessage>,
        tx: mpsc::UnboundedSender<AgentEvent>,
    ) {
        self.finish().await; // restore from previous if needed

        assert!(
            !self.is_streaming,
            "Agent is already streaming. Use steer() or follow_up()."
        );

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.is_streaming = true;

        // Move tools temporarily into context for the loop; restored after
        let mut context = AgentContext {
            system_prompt: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: std::mem::take(&mut self.tools),
        };

        let config = self.build_config();

        let _new_messages = agent_loop(messages, &mut context, &config, tx, cancel).await;

        self.tools = context.tools;
        self.messages = context.messages;
        self.is_streaming = false;
        self.cancel = None;
    }

    /// Continue from current context (for retries after errors). Returns a
    /// receiver immediately with the loop running concurrently.
    ///
    /// Call [`finish()`](Self::finish) after draining events to restore state.
    pub async fn continue_loop(&mut self) -> mpsc::UnboundedReceiver<AgentEvent> {
        self.finish().await; // restore from previous if needed

        let (tx, rx) = mpsc::unbounded_channel();

        if self.is_streaming {
            tracing::warn!("Agent is already streaming, skipping continue");
            return rx;
        }
        if self.messages.is_empty() {
            tracing::warn!("No messages to continue from, skipping continue");
            return rx;
        }

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.is_streaming = true;

        let mut context = AgentContext {
            system_prompt: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: std::mem::take(&mut self.tools),
        };

        let config = self.build_config();

        let handle = tokio::spawn(async move {
            let _new_messages = agent_loop_continue(&mut context, &config, tx, cancel).await;
            (context.tools, context.messages)
        });

        self.pending_completion = Some(handle);
        rx
    }

    /// Continue from current context, streaming events to a caller-provided sender.
    /// Blocks until the loop finishes and state is restored.
    pub async fn continue_loop_with_sender(&mut self, tx: mpsc::UnboundedSender<AgentEvent>) {
        self.finish().await; // restore from previous if needed

        if self.is_streaming {
            tracing::warn!("Agent is already streaming, skipping continue");
            return;
        }
        if self.messages.is_empty() {
            tracing::warn!("No messages to continue from, skipping continue");
            return;
        }

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.is_streaming = true;

        // Move tools temporarily into context for the loop; restored after
        let mut context = AgentContext {
            system_prompt: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: std::mem::take(&mut self.tools),
        };

        let config = self.build_config();

        let _new_messages = agent_loop_continue(&mut context, &config, tx, cancel).await;

        self.tools = context.tools;
        self.messages = context.messages;
        self.is_streaming = false;
        self.cancel = None;
    }

    /// Wait for the running agent loop to finish and restore state
    /// (messages, tools, streaming flag).
    ///
    /// Called automatically at the start of all prompting methods
    /// ([`prompt()`](Self::prompt), [`prompt_messages()`](Self::prompt_messages),
    /// [`prompt_messages_with_sender()`](Self::prompt_messages_with_sender),
    /// [`continue_loop()`](Self::continue_loop),
    /// [`continue_loop_with_sender()`](Self::continue_loop_with_sender)).
    /// Call explicitly when you need to access [`messages()`](Self::messages)
    /// right after draining events.
    pub async fn finish(&mut self) {
        if let Some(handle) = self.pending_completion.take() {
            match handle.await {
                Ok((tools, messages)) => {
                    self.tools = tools;
                    self.messages = messages;
                }
                Err(e) => {
                    // Task panicked or was cancelled — log and leave state as-is
                    tracing::error!("Agent loop task failed: {}", e);
                }
            }
            self.is_streaming = false;
            self.cancel = None;
        }
    }

    // -- Internal --

    fn build_config(&self) -> AgentLoopConfig {
        let steering_queue = self.steering_queue.clone();
        let steering_mode = self.steering_mode;

        let follow_up_queue = self.follow_up_queue.clone();
        let follow_up_mode = self.follow_up_mode;

        AgentLoopConfig {
            provider: self.provider.clone(),
            model: self.model.clone(),
            api_key: self.api_key.clone(),
            thinking_level: self.thinking_level,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            model_config: self.model_config.clone(),
            convert_to_llm: None,
            transform_context: None,
            get_steering_messages: Some(Box::new(move || {
                let mut queue = steering_queue.lock();
                match steering_mode {
                    QueueMode::OneAtATime => {
                        if queue.is_empty() {
                            vec![]
                        } else {
                            vec![queue.remove(0)]
                        }
                    }
                    QueueMode::All => queue.drain(..).collect(),
                }
            })),
            context_config: if self.context_management_disabled {
                None
            } else {
                Some(self.context_config.clone().unwrap_or_else(|| {
                    self.model_config
                        .as_ref()
                        .map(|m| ContextConfig::from_context_window(m.context_window))
                        .unwrap_or_default()
                }))
            },
            compaction_strategy: self.compaction_strategy.clone(),
            execution_limits: self.execution_limits.clone(),
            cache_config: self.cache_config.clone(),
            tool_execution: self.tool_execution.clone(),
            retry_config: self.retry_config.clone(),
            get_follow_up_messages: Some(Box::new(move || {
                let mut queue = follow_up_queue.lock();
                match follow_up_mode {
                    QueueMode::OneAtATime => {
                        if queue.is_empty() {
                            vec![]
                        } else {
                            vec![queue.remove(0)]
                        }
                    }
                    QueueMode::All => queue.drain(..).collect(),
                }
            })),
            before_turn: self.before_turn.clone(),
            after_turn: self.after_turn.clone(),
            on_error: self.on_error.clone(),
            input_filters: self.input_filters.clone(),
        }
    }
}
