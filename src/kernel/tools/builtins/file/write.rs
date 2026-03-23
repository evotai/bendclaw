use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::Impact;
use crate::kernel::OpType;
use crate::observability::log::slog;

/// Write file contents to the session workspace.
pub struct FileWriteTool;

impl FileWriteTool {
    fn extract_path(args: &serde_json::Value) -> &str {
        args.get("path").and_then(|v| v.as_str()).unwrap_or("")
    }
}

impl OperationClassifier for FileWriteTool {
    fn op_type(&self) -> OpType {
        OpType::FileWrite
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Medium)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        Self::extract_path(args).to_string()
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        ToolId::FileWrite.as_str()
    }

    fn description(&self) -> &str {
        "Write contents to a file. Overwrites the entire file. Use file_edit for partial changes."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (absolute or relative to working directory)"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
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

        let content = match args.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return Ok(ToolResult::error("Missing 'content' parameter")),
        };

        let full_path = match ctx.workspace.resolve_search_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path is not accessible")),
        };

        if let Some(parent) = full_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return Ok(ToolResult::error(format!(
                    "Failed to create parent directory: {e}"
                )));
            }
        }

        match tokio::fs::write(&full_path, content).await {
            Ok(()) => {
                slog!(debug, "file", "completed", path, bytes = content.len(),);
                Ok(ToolResult::ok(format!(
                    "Written {} bytes to {path}",
                    content.len()
                )))
            }
            Err(e) => {
                slog!(warn, "file", "failed", path, error = %e,);
                Ok(ToolResult::error(format!("Failed to write file: {e}")))
            }
        }
    }
}
