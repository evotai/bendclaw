use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;

/// Read file contents from the session workspace.
pub struct FileReadTool;

impl FileReadTool {
    fn extract_path(args: &serde_json::Value) -> &str {
        args.get("path").and_then(|v| v.as_str()).unwrap_or("")
    }
}

impl OperationClassifier for FileReadTool {
    fn op_type(&self) -> OpType {
        OpType::FileRead
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        Self::extract_path(args).to_string()
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        ToolId::FileRead.as_str()
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Accepts absolute paths or paths relative to the working directory."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (absolute or relative to working directory)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Missing 'path' parameter")),
        };

        let full_path = match ctx.workspace.resolve_search_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path is not accessible")),
        };

        match tokio::fs::read_to_string(&full_path).await {
            Ok(contents) => {
                tracing::info!(path, size_bytes = contents.len(), "file read succeeded");
                Ok(ToolResult::ok(contents))
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "file read failed");
                Ok(ToolResult::error(format!("Failed to read file: {e}")))
            }
        }
    }
}
