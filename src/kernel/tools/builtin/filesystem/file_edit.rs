use async_trait::async_trait;
use serde_json::json;

use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::kernel::Impact;
use crate::kernel::OpType;
use crate::types::Result;

/// Tool description for the LLM — the first source of behavioral guidance for file_edit.
const DESCRIPTION: &str = "\
Perform exact string replacements in files.\n\
\n\
Usage:\n\
- You must use file_read at least once in the conversation before editing. \
This tool will error if you attempt an edit without reading the file first.\n\
- When editing text from file_read output, ensure you preserve the exact indentation \
(tabs/spaces) as it appears AFTER the line number prefix. The line number prefix format is: \
line number + tab. Everything after that is the actual file content to match. \
Never include any part of the line number prefix in old_string or new_string.\n\
- ALWAYS prefer editing existing files in the codebase. NEVER write new files unless \
explicitly required.\n\
- Only use emojis if the user explicitly requests it. Avoid adding emojis to files \
unless asked.\n\
- The edit will FAIL if old_string is not unique in the file. Either provide a larger \
string with more surrounding context to make it unique, or use replace_all to change \
every instance of old_string.\n\
- Use replace_all for replacing and renaming strings across the file. This is useful \
if you want to rename a variable for instance.\n\
- Use this tool instead of shell sed/awk for file modifications.";

/// Parameter descriptions.
#[allow(dead_code)]
const PARAM_PATH: &str = "Absolute or workspace-relative path to the file to edit.";
#[allow(dead_code)]
const PARAM_OLD_STRING: &str = "The exact string to search for in the file. Must match exactly once unless replace_all is set.";
#[allow(dead_code)]
const PARAM_NEW_STRING: &str = "The replacement string.";

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file within the workspace"
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
        ToolId::Edit.as_str()
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

        let old_string = match args.get("old_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return Ok(ToolResult::error("Missing 'old_string' parameter")),
        };

        let new_string = match args.get("new_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return Ok(ToolResult::error("Missing 'new_string' parameter")),
        };

        let full_path = match ctx.workspace.resolve_safe_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path escapes workspace directory")),
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
                tracing::info!(path, "file edited");
                Ok(ToolResult::ok(format!("Edited {path} successfully")))
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "file edit failed");
                Ok(ToolResult::error(format!("Failed to write file: {e}")))
            }
        }
    }
}
