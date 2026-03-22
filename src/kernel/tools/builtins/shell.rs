use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::json;

use crate::base::truncate_chars_with_ellipsis;
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
        truncate_chars_with_ellipsis(Self::extract_command(args), 120)
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        ToolId::Shell.as_str()
    }

    fn description(&self) -> &str {
        "Execute shell commands. Use for running builds, tests, git operations, \
         API calls via CLI tools, and other command-line tasks. \
         Commands run in a subprocess with captured output."
    }

    fn hint(&self) -> &str {
        "execute a shell command"
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

        let output = ctx.workspace.exec(command, &HashMap::new()).await;

        let secret_ids = ctx.workspace.secret_variable_ids();
        if !secret_ids.is_empty() {
            let pool = ctx.pool.clone();
            crate::base::spawn_fire_and_forget("variable_touch_last_used", async move {
                let repo = VariableRepo::new(pool);
                let _ = repo.touch_last_used_many(&secret_ids).await;
            });
        }

        tracing::info!(
            stage = "shell",
            status = "completed",
            command,
            exit_code = output.exit_code,
            stdout_len = output.stdout.len(),
            stderr_len = output.stderr.len(),
            variable_count = ctx.workspace.build_env().len(),
            "shell completed"
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
