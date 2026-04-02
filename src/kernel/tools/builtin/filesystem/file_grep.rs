use async_trait::async_trait;
use serde_json::json;

use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::kernel::OpType;
use crate::types::Result;

/// Tool description for the LLM — the first source of behavioral guidance for grep.
const DESCRIPTION: &str = "\
A powerful search tool built on ripgrep.\n\
\n\
Usage:\n\
- ALWAYS use this tool for content search. NEVER invoke grep or rg as a shell command. \
This tool has been optimized for correct permissions and access.\n\
- Supports full regex syntax (e.g., \"log.*Error\", \"function\\\\s+\\\\w+\").\n\
- Filter files with file_pattern parameter (e.g., \"*.rs\", \"*.py\").\n\
- Respects .gitignore. Returns matching lines with file paths and line numbers.\n\
- Pattern syntax: Uses ripgrep (not grep) — literal braces need escaping \
(use `interface\\\\{\\\\}` to find `interface{}` in Go code).\n\
- For open-ended searches requiring multiple rounds, break the search into smaller queries.";

/// Parameter descriptions.
#[allow(dead_code)]
const PARAM_PATTERN: &str = "Regular expression pattern to search for.";
#[allow(dead_code)]
const PARAM_PATH: &str =
    "Absolute or relative path to search in. Defaults to the workspace directory.";
#[allow(dead_code)]
const PARAM_FILE_PATTERN: &str = "Optional glob to filter files (e.g. '*.rs', '*.py').";

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "Regular expression pattern to search for."
            },
            "path": {
                "type": "string",
                "description": "Absolute or relative path to search in. Defaults to the workspace directory."
            },
            "file_pattern": {
                "type": "string",
                "description": "Optional glob to filter files (e.g. '*.rs', '*.py')."
            }
        },
        "required": ["pattern"]
    })
}

const MAX_MATCHES: usize = 200;
const MAX_FILE_SIZE: u64 = 1_048_576; // 1MB

pub struct GrepTool;

impl OperationClassifier for GrepTool {
    fn op_type(&self) -> OpType {
        OpType::FileRead
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        format!("{pattern} in {path}")
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        ToolId::Grep.as_str()
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
        let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(ToolResult::error("Missing 'pattern' parameter")),
        };
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());

        let full_path = match ctx.workspace.resolve_safe_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path escapes workspace directory")),
        };

        let re = match regex::Regex::new(pattern) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("Invalid regex: {e}"))),
        };

        let glob = match file_pattern {
            Some(fp) => match globset::GlobBuilder::new(fp)
                .literal_separator(true)
                .build()
                .map(|g| globset::GlobSet::builder().add(g).build())
            {
                Ok(Ok(gs)) => Some(gs),
                _ => return Ok(ToolResult::error("Invalid file_pattern glob")),
            },
            None => None,
        };

        let workspace_root = ctx.workspace.dir().to_path_buf();
        let result = tokio::task::spawn_blocking(move || {
            let mut matches = Vec::new();
            let walker = ignore::WalkBuilder::new(&full_path)
                .hidden(true)
                .git_ignore(true)
                .build();

            for entry in walker.flatten() {
                if matches.len() >= MAX_MATCHES {
                    break;
                }
                let entry_path = entry.path();
                if !entry_path.is_file() {
                    continue;
                }
                if let Some(ref gs) = glob {
                    if !gs.is_match(entry_path.file_name().unwrap_or_default()) {
                        continue;
                    }
                }
                if entry
                    .metadata()
                    .map(|m| m.len() > MAX_FILE_SIZE)
                    .unwrap_or(true)
                {
                    continue;
                }
                let content = match std::fs::read_to_string(entry_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let display_path = entry_path
                    .strip_prefix(&workspace_root)
                    .unwrap_or(entry_path)
                    .display();
                for (i, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        matches.push(format!("{display_path}:{}:{line}", i + 1));
                        if matches.len() >= MAX_MATCHES {
                            break;
                        }
                    }
                }
            }
            matches
        })
        .await
        .unwrap_or_default();

        if result.is_empty() {
            let hint = if path == "." {
                "No matches found. The default search path is the workspace directory (not user home). Try providing an absolute path."
            } else {
                "No matches found. Verify the path exists, or try searching a parent directory."
            };
            return Ok(ToolResult::ok(hint));
        }

        let truncated = result.len() >= MAX_MATCHES;
        let mut output = result.join("\n");
        if truncated {
            output.push_str(&format!("\n\n(truncated at {MAX_MATCHES} matches)"));
        }
        tracing::info!(pattern, path, matches = result.len(), "grep completed");
        Ok(ToolResult::ok(output))
    }
}
