use async_trait::async_trait;
use serde_json::json;

use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::kernel::OpType;
use crate::types::Result;

/// Tool description for the LLM — the first source of behavioral guidance for file_read.
const DESCRIPTION: &str = "\
Read a text file from the local filesystem. You can access any file directly by using this tool.\n\
Assume this tool is able to read all files on the machine. If the User provides a path \
to a file assume that path is valid. It is okay to read a file that does not exist; \
an error will be returned.\n\
\n\
Usage:\n\
- The path parameter must be an absolute path, not a relative path.\n\
- Use this tool instead of shell cat/head/tail for reading files.\n\
- This tool reads the entire file at once.\n\
- This tool can only read text files, not directories or binary files (images, PDFs, etc.). \
To list a directory, use list_dir.\n\
- If you read a file that exists but has empty contents you will receive a warning \
in place of file contents.";

/// Parameter descriptions.
#[allow(dead_code)]
const PARAM_PATH: &str = "Absolute or workspace-relative path to the file to read.";

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file within the workspace"
            }
        },
        "required": ["path"]
    })
}

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
        ToolId::Read.as_str()
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

        let full_path = match ctx.workspace.resolve_safe_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path escapes workspace directory")),
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
