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

/// List directory contents within the session workspace.
pub struct ListDirTool;

impl ListDirTool {
    fn extract_path(args: &serde_json::Value) -> &str {
        args.get("path").and_then(|v| v.as_str()).unwrap_or("")
    }
}

impl OperationClassifier for ListDirTool {
    fn op_type(&self) -> OpType {
        OpType::FileList
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        Self::extract_path(args).to_string()
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        ToolId::ListDir.as_str()
    }

    fn description(&self) -> &str {
        "List the contents of a directory. Accepts absolute or workspace-relative paths."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory within the workspace"
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

        let full_path = match ctx.workspace.resolve_safe_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path escapes workspace directory")),
        };

        let mut read_dir = match tokio::fs::read_dir(&full_path).await {
            Ok(rd) => rd,
            Err(e) => {
                tracing::warn!(path, error = %e, "list_dir failed");
                return Ok(ToolResult::error(format!("Failed to read directory: {e}")));
            }
        };

        let mut entries = Vec::new();
        loop {
            match read_dir.next_entry().await {
                Ok(Some(entry)) => {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let meta = entry.metadata().await;
                    let line = match meta {
                        Ok(m) if m.is_dir() => format!("{name}/"),
                        Ok(m) => format!("{name} ({} bytes)", m.len()),
                        Err(_) => name,
                    };
                    entries.push(line);
                }
                Ok(None) => break,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "Failed reading directory entry: {e}"
                    )))
                }
            }
        }

        entries.sort();
        let output = entries.join("\n");
        tracing::info!(path, count = entries.len(), "list_dir succeeded");
        Ok(ToolResult::ok(output))
    }
}
