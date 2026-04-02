use async_trait::async_trait;
use serde_json::json;

use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::kernel::OpType;
use crate::types::Result;

/// Tool description for the LLM — the first source of behavioral guidance for glob.
const DESCRIPTION: &str = "\
Fast file pattern matching tool that works with any codebase size.\n\
\n\
Usage:\n\
- ALWAYS use this tool to find files. NEVER use shell with find or ls for file discovery.\n\
- Supports glob patterns like \"**/*.rs\" or \"src/**/*.ts\".\n\
- Returns matching file paths sorted by modification time.\n\
- Respects .gitignore.\n\
- Use this tool when you need to find files by name patterns.\n\
- For open-ended searches requiring multiple rounds of globbing and grepping, \
break the search into smaller targeted queries.";

/// Parameter descriptions.
#[allow(dead_code)]
const PARAM_PATTERN: &str =
    "Glob pattern to match file names, e.g. '*.rs', '*.test.ts', 'Cargo.toml'.";
#[allow(dead_code)]
const PARAM_PATH: &str =
    "Absolute or relative path to search in. Defaults to the workspace directory.";

fn schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "Glob pattern to match file names, e.g. '*.rs', '*.test.ts', 'Cargo.toml'."
            },
            "path": {
                "type": "string",
                "description": "Absolute or relative path to search in. Defaults to the workspace directory."
            }
        },
        "required": ["pattern"]
    })
}

const MAX_RESULTS: usize = 500;

pub struct GlobTool;

impl OperationClassifier for GlobTool {
    fn op_type(&self) -> OpType {
        OpType::FileList
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        ToolId::Glob.as_str()
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

        let full_path = match ctx.workspace.resolve_safe_path(path) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Path escapes workspace directory")),
        };

        let gs = match globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map(|g| globset::GlobSet::builder().add(g).build())
        {
            Ok(Ok(gs)) => gs,
            _ => return Ok(ToolResult::error("Invalid glob pattern")),
        };

        let workspace_root = ctx.workspace.dir().to_path_buf();
        let result = tokio::task::spawn_blocking(move || {
            let mut matches = Vec::new();
            let walker = ignore::WalkBuilder::new(&full_path)
                .hidden(true)
                .git_ignore(true)
                .build();

            for entry in walker.flatten() {
                if matches.len() >= MAX_RESULTS {
                    break;
                }
                let entry_path = entry.path();
                if !entry_path.is_file() {
                    continue;
                }
                if gs.is_match(entry_path.file_name().unwrap_or_default()) {
                    let display = entry_path
                        .strip_prefix(&workspace_root)
                        .unwrap_or(entry_path)
                        .display()
                        .to_string();
                    matches.push(display);
                }
            }
            matches.sort();
            matches
        })
        .await
        .unwrap_or_default();

        if result.is_empty() {
            let hint = if path == "." {
                "No files found. The default search path is the workspace directory (not user home). Try providing an absolute path."
            } else {
                "No files found. Verify the path exists, or try searching a parent directory."
            };
            return Ok(ToolResult::ok(hint));
        }

        let truncated = result.len() >= MAX_RESULTS;
        let mut output = result.join("\n");
        if truncated {
            output.push_str(&format!("\n\n(truncated at {MAX_RESULTS} results)"));
        }
        tracing::info!(pattern, path, files = result.len(), "glob completed");
        Ok(ToolResult::ok(output))
    }
}
