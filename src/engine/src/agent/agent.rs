use std::collections::HashMap;
use std::sync::Arc;

use bend_base::logx;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::api::ApiClient;
use crate::api::ProviderKind;
use crate::costtracker::CostTracker;
use crate::hooks::HookConfig;
use crate::mcp::McpClient;
use crate::mcp::{self};
use crate::tools::ToolRegistry;
use crate::types::*;

/// Configuration options for creating an agent.
#[derive(Default)]
pub struct AgentOptions {
    /// LLM model ID (default: claude-sonnet-4-6-20250514).
    pub model: Option<String>,
    /// API key for authentication.
    pub api_key: Option<String>,
    /// Custom API base URL.
    pub base_url: Option<String>,
    pub provider: Option<ProviderKind>,
    /// Working directory for tool execution.
    pub cwd: Option<String>,
    /// Custom system prompt (replaces default).
    pub system_prompt: Option<String>,
    /// Additional text appended to the default system prompt.
    pub append_system_prompt: Option<String>,
    /// Maximum number of agentic turns (default: 100).
    pub max_turns: Option<u32>,
    /// Maximum budget in USD.
    pub max_budget_usd: Option<f64>,
    /// Permission mode for tool access control.
    pub permission_mode: Option<PermissionMode>,
    /// Tool names to allow (whitelist).
    pub allowed_tools: Option<Vec<String>>,
    /// Tool names to deny (blacklist).
    pub disallowed_tools: Option<Vec<String>>,
    /// Custom permission callback.
    pub can_use_tool: Option<CanUseToolFn>,
    /// Custom tools to register.
    pub custom_tools: Vec<Arc<dyn Tool>>,
    /// MCP server configurations.
    pub mcp_servers: HashMap<String, McpServerConfig>,
    /// Subagent definitions.
    pub agents: HashMap<String, SubagentDefinition>,
    /// Extended thinking configuration.
    pub thinking: Option<ThinkingConfig>,
    /// Maximum output tokens per response.
    pub max_tokens: Option<u64>,
    /// Structured output JSON schema.
    pub json_schema: Option<Value>,
    /// Hook configuration.
    pub hooks: Option<HookConfig>,
    /// Custom HTTP headers for API requests.
    pub custom_headers: HashMap<String, String>,
    /// Resume the most recent session in the current working directory.
    pub continue_session: Option<bool>,
    /// Resume a specific session by ID.
    pub resume: Option<String>,
    /// Fork instead of continuing a session.
    pub fork_session: Option<bool>,
    /// Control whether the session is persisted to disk.
    pub persist_session: Option<bool>,
    /// Explicit session ID to use (instead of auto-generating one).
    pub session_id: Option<String>,
    /// Effort level for reasoning: "low", "medium", "high", "max".
    pub effort: Option<String>,
    /// Fallback model if primary is unavailable.
    pub fallback_model: Option<String>,
    /// Sandbox configuration for restricting tool execution.
    pub sandbox: Option<SandboxSettings>,
    /// Enable debug mode.
    pub debug: Option<bool>,
    /// Additional working directories.
    pub additional_directories: Option<Vec<String>>,
}

/// Subagent definition.
#[derive(Debug, Clone)]
pub struct SubagentDefinition {
    pub description: String,
    pub instructions: Option<String>,
    pub tools: Option<Vec<String>>,
    pub model: Option<String>,
}

/// The main Agent struct that orchestrates the agentic loop.
pub struct Agent {
    pub(crate) api_client: ApiClient,
    pub(crate) registry: ToolRegistry,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Working directory.
    pub cwd: String,
    /// Custom system prompt.
    pub system_prompt: Option<String>,
    /// Additional system prompt text.
    pub append_system_prompt: Option<String>,
    /// Maximum agentic turns.
    pub max_turns: u32,
    /// Maximum budget in USD.
    pub max_budget_usd: Option<f64>,
    pub(crate) cost_tracker: CostTracker,
    pub(crate) mcp_client: Arc<McpClient>,
    /// Subagent definitions.
    pub agents: HashMap<String, SubagentDefinition>,
    /// Extended thinking configuration.
    pub thinking: Option<ThinkingConfig>,
    /// Maximum output tokens.
    pub max_tokens: Option<u64>,
    /// Structured output JSON schema.
    pub json_schema: Option<Value>,
    /// Hook configuration.
    pub hooks: HookConfig,
    pub(crate) can_use_tool: Option<CanUseToolFn>,
    pub(crate) session_id: String,
}

impl Agent {
    /// Create a new agent with the given options.
    pub async fn new(options: AgentOptions) -> Result<Self, String> {
        let cwd = match options.cwd {
            Some(cwd) => cwd,
            None => std::env::current_dir()
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .to_string(),
        };
        let provider = options.provider.clone().unwrap_or(ProviderKind::Anthropic);
        let api_client = ApiClient::with_provider(
            provider,
            options.api_key.clone(),
            options.base_url.clone(),
            options.model.clone(),
            options.custom_headers.clone(),
        )
        .map_err(|error| error.to_string())?;

        let mut registry = ToolRegistry::default_registry();

        // Register custom tools
        for tool in options.custom_tools {
            registry.register(tool);
        }

        // Apply allowed/disallowed tool filters
        if let Some(allowed) = &options.allowed_tools {
            let allowed_refs: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
            registry.retain(&allowed_refs);
        }
        if let Some(disallowed) = &options.disallowed_tools {
            let disallowed_refs: Vec<&str> = disallowed.iter().map(|s| s.as_str()).collect();
            registry.remove(&disallowed_refs);
        }

        // Connect MCP servers
        let mcp_client = Arc::new(McpClient::new());
        for (name, config) in &options.mcp_servers {
            match mcp_client.connect(name, config.clone()).await {
                Ok(tools) => {
                    let mcp_tools = mcp::create_mcp_tools(name, &tools, mcp_client.clone());
                    for tool in mcp_tools {
                        registry.register(tool);
                    }
                }
                Err(e) => {
                    logx!(
                        warn,
                        "mcp",
                        "connect_failed",
                        server = %name,
                        error = %e,
                    );
                }
            }
        }

        let session_id = options
            .session_id
            .clone()
            .unwrap_or_else(crate::ids::new_session_id);

        Ok(Self {
            api_client,
            registry,
            messages: Vec::new(),
            cwd,
            system_prompt: options.system_prompt,
            append_system_prompt: options.append_system_prompt,
            max_turns: options.max_turns.unwrap_or(100),
            max_budget_usd: options.max_budget_usd,
            cost_tracker: CostTracker::new(),
            mcp_client,
            agents: options.agents,
            thinking: options.thinking,
            max_tokens: options.max_tokens,
            json_schema: options.json_schema,
            hooks: options.hooks.unwrap_or_default(),
            can_use_tool: options.can_use_tool,
            session_id,
        })
    }

    /// Execute a query and return a channel of streaming SDK messages.
    pub async fn query(
        &mut self,
        prompt: &str,
    ) -> (
        mpsc::Receiver<SDKMessage>,
        tokio::task::JoinHandle<Vec<Message>>,
    ) {
        let (tx, rx) = mpsc::channel(100);

        // Add user message
        let user_msg = crate::utils::messages::create_user_message(prompt);
        self.messages.push(user_msg);

        // Clone what we need for the async loop
        let messages = self.messages.clone();
        let api_client = self.api_client.clone();
        let cwd = self.cwd.clone();
        let system_prompt = self.system_prompt.clone();
        let append_system_prompt = self.append_system_prompt.clone();
        let max_turns = self.max_turns;
        let max_budget_usd = self.max_budget_usd;
        let cost_tracker = self.cost_tracker.clone();
        let thinking = self.thinking.clone();
        let max_tokens = self.max_tokens;
        let registry = std::sync::Arc::new(self.registry_snapshot());
        let can_use_tool = self.can_use_tool.clone();

        let handle = tokio::spawn(async move {
            match super::r#loop::run_loop(
                api_client,
                messages,
                registry,
                &cwd,
                system_prompt.as_deref(),
                append_system_prompt.as_deref(),
                max_turns,
                max_budget_usd,
                &cost_tracker,
                thinking,
                max_tokens,
                can_use_tool.as_ref(),
                tx.clone(),
            )
            .await
            {
                Ok(final_messages) => final_messages,
                Err(e) => {
                    let _ = tx
                        .send(SDKMessage::Error {
                            message: e.to_string(),
                        })
                        .await;
                    Vec::new()
                }
            }
        });

        (rx, handle)
    }

    /// Blocking query that returns a complete QueryResult.
    pub async fn prompt(&mut self, text: &str) -> Result<QueryResult, String> {
        let start = std::time::Instant::now();
        let (mut rx, handle) = self.query(text).await;

        let mut result_text = String::new();
        let mut usage = Usage::default();
        let mut num_turns = 0;
        let mut cost_usd = 0.0;
        let mut all_messages = Vec::new();
        let mut summary = RunSummary::default();

        while let Some(msg) = rx.recv().await {
            match msg {
                SDKMessage::Result {
                    text,
                    usage: u,
                    num_turns: t,
                    cost_usd: c,
                    messages,
                    summary: s,
                    ..
                } => {
                    result_text = text;
                    usage = u;
                    num_turns = t;
                    cost_usd = c;
                    all_messages = messages;
                    summary = s;
                }
                SDKMessage::Assistant { message, .. } => {
                    let text = crate::types::extract_text(&message);
                    if !text.is_empty() {
                        result_text = text;
                    }
                    all_messages.push(message);
                }
                SDKMessage::Error { message } => {
                    return Err(message);
                }
                _ => {}
            }
        }

        let _ = handle.await;
        let duration_ms = start.elapsed().as_millis() as u64;

        // Update agent's message history
        self.messages = all_messages.clone();

        Ok(QueryResult {
            text: result_text,
            usage,
            num_turns,
            cost_usd,
            duration_ms,
            messages: all_messages,
            summary,
        })
    }

    /// Get current conversation messages.
    pub fn get_messages(&self) -> &[Message] {
        &self.messages
    }

    /// Clear conversation history.
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Set the model.
    pub fn set_model(&mut self, model: &str) {
        self.api_client.set_model(model.to_string());
    }

    /// Get the current model.
    pub fn model(&self) -> &str {
        self.api_client.model()
    }

    /// Get the cost tracker.
    pub fn cost_tracker(&self) -> &CostTracker {
        &self.cost_tracker
    }

    /// Get session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Close the agent and clean up resources.
    pub async fn close(&self) {
        self.mcp_client.close_all().await;
    }

    /// Create a snapshot of the current registry for use in async context.
    fn registry_snapshot(&self) -> ToolRegistry {
        let mut snapshot = ToolRegistry::new();
        for tool in self.registry.all() {
            snapshot.register(tool);
        }
        snapshot
    }
}
