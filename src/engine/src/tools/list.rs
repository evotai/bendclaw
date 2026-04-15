//! List files tool — directory exploration.

use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

use crate::types::*;

/// List files and directories. Uses `find` or `fd` for efficient traversal.
pub struct ListFilesTool {
    pub max_results: usize,
    pub timeout: Duration,
}

impl Default for ListFilesTool {
    fn default() -> Self {
        Self {
            max_results: 200,
            timeout: Duration::from_secs(10),
        }
    }
}

impl ListFilesTool {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentTool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn label(&self) -> &str {
        "List Files"
    }

    fn description(&self) -> &str {
        "List the contents of a directory. Use to explore project structure before reading specific files.\n\
         \n\
         Usage:\n\
         - Use this tool instead of shell ls or find for directory listing.\n\
         - Optionally filter by glob pattern (e.g., '*.rs').\n\
         - Excludes common noise directories (target, .git, node_modules) by default.\n\
         - If your command will create new directories or files, first use this tool to verify the \
         parent directory exists and is the correct location."
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let path = params["path"].as_str().unwrap_or(".");
        let max_depth = params["max_depth"].as_u64().unwrap_or(3);
        let pattern = params["pattern"].as_str();

        let mut parts = vec![
            "find".into(),
            path.to_string(),
            format!("-maxdepth {max_depth}"),
        ];
        if let Some(pat) = pattern {
            parts.push(format!("-name {pat}"));
        }
        parts.push(r#"-not -path "*/target/*""#.into());
        parts.push(r#"-not -path "*/.git/*""#.into());
        parts.push(r#"-not -path "*/node_modules/*""#.into());
        parts.push("-type f".into());

        Some(parts.join(" "))
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list (default: current directory)"
                },
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to filter files, e.g. '*.rs' (optional)"
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum directory depth (default: 3)"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let cancel = ctx.cancel;
        let path_str = params["path"].as_str().unwrap_or(".");
        let pattern = params["pattern"].as_str();
        let max_depth = params["max_depth"].as_u64().unwrap_or(3);

        let path = ctx
            .path_guard
            .resolve_optional_path(&ctx.cwd, Some(path_str))?;

        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Check path exists
        if !path.exists() {
            return Err(ToolError::Failed(format!(
                "Directory not found: {}. Check the path and try again.",
                path.display()
            )));
        }

        let mut cmd = Command::new("find");
        cmd.arg(&path);
        cmd.args(["-maxdepth", &max_depth.to_string()]);

        if let Some(pat) = pattern {
            cmd.args(["-name", pat]);
        }

        // Exclude common noise
        cmd.args(["-not", "-path", "*/target/*"]);
        cmd.args(["-not", "-path", "*/.git/*"]);
        cmd.args(["-not", "-path", "*/node_modules/*"]);

        cmd.arg("-type").arg("f");
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let timeout = self.timeout;

        let result = tokio::select! {
            _ = cancel.cancelled() => return Err(ToolError::Cancelled),
            _ = tokio::time::sleep(timeout) => return Err(ToolError::Failed("Listing timed out".into())),
            result = cmd.output() => {
                result.map_err(|e| ToolError::Failed(format!("Failed to list: {}", e)))?
            }
        };

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let mut lines: Vec<&str> = stdout.lines().collect();
        lines.sort();

        let total = lines.len();
        let truncated = total > self.max_results;
        if truncated {
            lines.truncate(self.max_results);
        }

        let text = if lines.is_empty() {
            format!("No files found in {}", path.display())
        } else if truncated {
            format!(
                "{}\n\n... ({} files, showing first {})",
                lines.join("\n"),
                total,
                self.max_results
            )
        } else {
            format!("{}\n\n({} files)", lines.join("\n"), total)
        };

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::json!({ "total": total, "truncated": truncated }),
            retention: Retention::Normal,
        })
    }
}
