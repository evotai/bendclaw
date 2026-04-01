use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::tools::execution::tool_context::ToolContext;
use crate::kernel::tools::execution::tool_contract::OperationClassifier;
use crate::kernel::tools::execution::tool_contract::Tool;
use crate::kernel::tools::execution::tool_contract::ToolResult;
use crate::kernel::tools::execution::tool_id::ToolId;
use crate::kernel::Impact;
use crate::kernel::OpType;

/// Tool description for the LLM — the first source of behavioral guidance for file_write.
const DESCRIPTION: &str = "\
Write contents to a file on the local filesystem.\n\
\n\
Usage:\n\
- This tool will overwrite the existing file if there is one at the provided path.\n\
- If this is an existing file, you MUST use file_read first to read the file's contents. \
This tool will fail if you did not read the file first.\n\
- Prefer file_edit for modifying existing files — it only sends the diff. \
Only use this tool to create new files or for complete rewrites.\n\
- NEVER create documentation files (*.md) or README files unless explicitly requested by the User.\n\
- Only use emojis if the user explicitly requests it. Avoid writing emojis to files unless asked.\n\
- Accepts absolute or workspace-relative paths. Creates parent directories as needed.";

/// Parameter descriptions.
#[allow(dead_code)]
const PARAM_PATH: &str = "Absolute or workspace-relative path to the file to write.";
#[allow(dead_code)]
const PARAM_CONTENT: &str = "Content to write to the file.";

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file within the workspace"
            },
            "content": {
                "type": "string",
                "description": "Content to write to the file"
            }
        },
        "required": ["path", "content"]
    })
}

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
        ToolId::Write.as_str()
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters_schema(&self) -> serde_json::Value {
        schema()
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

        let full_path = match ctx.workspace.resolve_safe_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path escapes workspace directory")),
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
                tracing::info!(path, bytes = content.len(), "file written");
                Ok(ToolResult::ok(format!(
                    "Written {} bytes to {path}",
                    content.len()
                )))
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "file write failed");
                Ok(ToolResult::error(format!("Failed to write file: {e}")))
            }
        }
    }
}
