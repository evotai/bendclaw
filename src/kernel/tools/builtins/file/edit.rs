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

/// Search-and-replace edit within a file in the session workspace.
pub struct FileEditTool;

impl FileEditTool {
    fn extract_path(args: &serde_json::Value) -> &str {
        args.get("path").and_then(|v| v.as_str()).unwrap_or("")
    }
}

impl OperationClassifier for FileEditTool {
    fn op_type(&self) -> OpType {
        OpType::Edit
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Medium)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        Self::extract_path(args).to_string()
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        ToolId::FileEdit.as_str()
    }

    fn description(&self) -> &str {
        "Apply a search-and-replace edit to a file. Accepts absolute paths or paths relative to the working directory."
    }

    fn hint(&self) -> &str {
        "edit a file by string replacement"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (absolute or relative to working directory)"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact string to search for in the file"
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement string"
                }
            },
            "required": ["path", "old_string", "new_string"]
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

        let old_string = match args.get("old_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return Ok(ToolResult::error("Missing 'old_string' parameter")),
        };

        let new_string = match args.get("new_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return Ok(ToolResult::error("Missing 'new_string' parameter")),
        };

        let full_path = match ctx.workspace.resolve_search_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path is not accessible")),
        };

        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read file: {e}"))),
        };

        let count = content.matches(old_string).count();
        if count == 0 {
            return Ok(ToolResult::error(format!("old_string not found in {path}")));
        }
        if count > 1 {
            return Ok(ToolResult::error(format!(
                "old_string found {count} times in {path} — must be unique"
            )));
        }

        let new_content = content.replacen(old_string, new_string, 1);

        match tokio::fs::write(&full_path, &new_content).await {
            Ok(()) => {
                tracing::info!(
                    stage = "file_edit",
                    status = "completed",
                    path,
                    "file_edit completed"
                );
                Ok(ToolResult::ok(format!("Edited {path} successfully")))
            }
            Err(e) => {
                tracing::warn!(stage = "file_edit", status = "failed", path, error = %e, "file_edit failed");
                Ok(ToolResult::error(format!("Failed to write file: {e}")))
            }
        }
    }
}
