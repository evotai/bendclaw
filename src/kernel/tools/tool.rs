use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

pub use super::context::ToolContext;
pub use super::context::ToolRuntime;
use crate::base::Result;
use crate::kernel::Impact;
use crate::kernel::OpType;

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
