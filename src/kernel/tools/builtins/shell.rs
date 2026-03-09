use std::collections::HashMap;

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
use crate::storage::dal::variable::VariableRepo;

/// Execute a shell command in the session workspace directory.
/// Zero-field struct — workspace is obtained from `ctx` at execution time.
pub struct ShellTool;

impl ShellTool {
    fn extract_command(args: &serde_json::Value) -> &str {
        args.get("command").and_then(|v| v.as_str()).unwrap_or("")
    }
}

const READONLY_PREFIXES: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "grep",
    "find",
    "echo",
    "pwd",
    "env",
    "git status",
    "git log",
    "git diff",
    "git show",
    "git branch",
];

const DESTRUCTIVE_PATTERNS: &[&str] = &["rm ", "rm\t", "git push", "sudo", "docker", "kubectl"];

impl OperationClassifier for ShellTool {
    fn op_type(&self) -> OpType {
        OpType::Execute
    }

    fn classify_impact(&self, args: &serde_json::Value) -> Option<Impact> {
        let command = Self::extract_command(args).trim();
        if DESTRUCTIVE_PATTERNS.iter().any(|p| command.contains(p)) {
            Some(Impact::High)
        } else if READONLY_PREFIXES.iter().any(|p| command.starts_with(p)) {
            Some(Impact::Low)
        } else {
            Some(Impact::Medium)
        }
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let cmd = Self::extract_command(args);
        if cmd.len() > 120 {
            format!("{}...", &cmd[..117])
        } else {
            cmd.to_string()
        }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        ToolId::Shell.as_str()
    }

    fn description(&self) -> &str {
        "Execute a shell command in the workspace directory."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let command = match args.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return Ok(ToolResult::error("Missing 'command' parameter")),
        };

        // Load variables and build extra env map
        let repo = VariableRepo::new(ctx.pool.clone());
        let variables = match repo.list_all().await {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult::error(format!("Failed to load variables: {e}")));
            }
        };

        let extra: HashMap<String, String> = variables
            .iter()
            .map(|v| (v.key.clone(), v.value.clone()))
            .collect();

        let output = ctx.workspace.exec(command, &extra).await;

        // Fire-and-forget: update last_used_at for all variables
        if !variables.is_empty() {
            let pool = ctx.pool.clone();
            tokio::spawn(async move {
                let repo = VariableRepo::new(pool);
                for v in &variables {
                    let _ = repo.touch_last_used(&v.id).await;
                }
            });
        }

        tracing::info!(
            command,
            exit_code = output.exit_code,
            stdout_len = output.stdout.len(),
            stderr_len = output.stderr.len(),
            variable_count = extra.len(),
            "shell command executed"
        );

        Ok(ToolResult {
            success: output.exit_code == 0,
            output: output.stdout,
            error: if output.stderr.is_empty() {
                None
            } else {
                Some(output.stderr)
            },
        })
    }
}
