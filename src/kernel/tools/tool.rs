use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::base::Result;
use crate::kernel::run::event::Event;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::tools::cli_agent::SharedAgentState;
use crate::kernel::Impact;
use crate::kernel::OpType;
use crate::storage::pool::Pool;

/// Runtime controls injected into tools during execution.
#[derive(Clone)]
pub struct ToolRuntime {
    pub event_tx: Option<mpsc::Sender<Event>>,
    pub cancel: CancellationToken,
    pub cli_agent_state: SharedAgentState,
    pub tool_call_id: Option<Arc<str>>,
}

use crate::kernel::writer::tool_op::ToolWriter;

/// Per-session identity context passed to tools at execution time.
#[derive(Clone)]
pub struct ToolContext {
    pub user_id: Arc<str>,
    pub session_id: Arc<str>,
    pub agent_id: Arc<str>,
    pub run_id: Arc<str>,
    pub trace_id: Arc<str>,
    pub workspace: Arc<Workspace>,
    pub pool: Pool,
    /// True when this run was dispatched from a remote node (has parent_run_id).
    /// Prevents nested dispatch — only one level of fanout is allowed.
    pub is_dispatched: bool,
    pub runtime: ToolRuntime,
    pub tool_writer: ToolWriter,
}

impl ToolContext {
    pub fn current_tool_call_id(&self) -> &str {
        match &self.runtime.tool_call_id {
            Some(tool_call_id) => tool_call_id,
            None => &self.run_id,
        }
    }
}

/// Result of an in-process tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl ToolResult {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self {
            success: false,
            output: String::new(),
            error: Some(msg),
        }
    }
}

/// LLM-facing tool description, auto-generated from the trait.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Classifies a tool operation for the agent message timeline.
pub trait OperationClassifier {
    fn op_type(&self) -> OpType;

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        None
    }

    fn summarize(&self, args: &serde_json::Value) -> String;
}

/// In-process tool that the agent loop can call directly.
#[async_trait]
pub trait Tool: OperationClassifier + Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult>;

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }
}
