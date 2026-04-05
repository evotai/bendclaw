use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::RwLock;

/// Tool input schema following JSON Schema format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(default)]
    pub properties: HashMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(default, rename = "additionalProperties")]
    pub additional_properties: Option<bool>,
}

impl Default for ToolInputSchema {
    fn default() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: HashMap::new(),
            required: Vec::new(),
            additional_properties: Some(false),
        }
    }
}

/// Result returned by a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    #[serde(default)]
    pub content: Vec<ToolResultContent>,
    #[serde(default)]
    pub is_error: bool,
}

impl ToolResult {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolResultContent::Text { text: text.into() }],
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolResultContent::Text { text: text.into() }],
            is_error: true,
        }
    }

    pub fn image(data: String, media_type: String) -> Self {
        Self {
            content: vec![ToolResultContent::Image {
                source: ImageSource {
                    source_type: "base64".to_string(),
                    media_type,
                    data,
                },
            }],
            is_error: false,
        }
    }

    pub fn get_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                ToolResultContent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolResultContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// Context passed to tools during execution.
#[derive(Clone)]
pub struct ToolUseContext {
    pub working_dir: String,
    pub abort_signal: tokio_util::sync::CancellationToken,
    pub read_file_state: Arc<RwLock<HashMap<String, String>>>,
}

impl ToolUseContext {
    pub fn new(working_dir: String) -> Self {
        Self {
            working_dir,
            abort_signal: tokio_util::sync::CancellationToken::new(),
            read_file_state: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl fmt::Debug for ToolUseContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolUseContext")
            .field("working_dir", &self.working_dir)
            .finish()
    }
}

/// Permission decision for tool execution.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionDecision {
    Allow,
    Deny(String),
    AllowWithModifiedInput(Value),
}

/// Callback function for custom permission logic.
pub type CanUseToolFn = Arc<dyn Fn(&str, &Value) -> PermissionDecision + Send + Sync>;

/// The core trait that all tools must implement.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> ToolInputSchema;

    async fn call(&self, input: Value, context: &ToolUseContext) -> Result<ToolResult, ToolError>;

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    fn is_concurrency_safe(&self, input: &Value) -> bool {
        self.is_read_only(input)
    }
}

/// Errors that can occur during tool execution.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool execution failed: {0}")]
    ExecutionError(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Tool aborted")]
    Aborted,
}

/// Sandbox configuration for restricting tool execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxSettings {
    /// Whether sandboxing is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Network restrictions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<SandboxNetworkConfig>,
    /// Filesystem restrictions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<SandboxFilesystemConfig>,
}

/// Network configuration for the sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxNetworkConfig {
    /// Domains allowed for network access.
    #[serde(default)]
    pub allowed_domains: Vec<String>,
}

/// Filesystem configuration for the sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxFilesystemConfig {
    /// Paths where writes are denied.
    #[serde(default)]
    pub deny_write: Vec<String>,
    /// Paths where reads are denied.
    #[serde(default)]
    pub deny_read: Vec<String>,
}

/// Permission mode for controlling tool access.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    #[default]
    BypassPermissions,
    Plan,
    DontAsk,
    Auto,
}
