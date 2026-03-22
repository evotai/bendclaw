use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;

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
        "Find files by name pattern. Returns matching file paths relative to workspace."
    }

    fn hint(&self) -> &str {
        "find files by name pattern — prefer over shell"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match file names, e.g. '*.rs', '*.test.ts', 'Cargo.toml'"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (relative to working directory, default: '.')"
                }
            },
            "required": ["pattern"]
        })
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

        let full_path = match ctx.workspace.resolve_search_path(path) {
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
            return Ok(ToolResult::ok("No files found."));
        }

        let truncated = result.len() >= MAX_RESULTS;
        let mut output = result.join("\n");
        if truncated {
            output.push_str(&format!("\n\n(truncated at {MAX_RESULTS} results)"));
        }
        tracing::info!(
            stage = "glob",
            status = "completed",
            pattern,
            path,
            files = result.len(),
            "glob completed"
        );
        Ok(ToolResult::ok(output))
    }
}
