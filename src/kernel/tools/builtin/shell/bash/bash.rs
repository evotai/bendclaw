use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::base::truncate_chars_with_ellipsis;
use crate::base::Result;
use crate::kernel::tools::execution::tool_context::ToolContext;
use crate::kernel::tools::execution::tool_contract::OperationClassifier;
use crate::kernel::tools::execution::tool_contract::Tool;
use crate::kernel::tools::execution::tool_contract::ToolResult;
use crate::kernel::tools::execution::tool_id::ToolId;
use crate::kernel::tools::execution::tool_services::SecretUsageSink;
use crate::kernel::Impact;
use crate::kernel::OpType;
use crate::observability::log::slog;

const DESCRIPTION: &str = "\
Execute a shell command and return its output.\n\
\n\
The working directory persists between commands, but shell state does not.\n\
\n\
IMPORTANT: Avoid using this tool to run grep, find, cat, head, tail, sed, awk, or echo \
commands, unless explicitly instructed or after you have verified that a dedicated tool \
cannot accomplish your task. Instead, use the appropriate dedicated tool:\n\
\n\
- File search: Use glob (NOT find)\n\
- Directory listing: Use list_dir (NOT ls)\n\
- Content search: Use grep tool (NOT shell grep or rg)\n\
- Read files: Use file_read (NOT cat/head/tail)\n\
- Edit files: Use file_edit (NOT sed/awk)\n\
- Write files: Use file_write (NOT echo/cat redirection)\n\
\n\
The built-in tools provide a better experience and make it easier to review operations.\n\
\n\
# Instructions\n\
- If your command will create new directories or files, first use list_dir to verify the \
parent directory exists and is the correct location.\n\
- Always quote file paths that contain spaces with double quotes.\n\
- Try to maintain your current working directory by using absolute paths and avoiding cd.\n\
- When issuing multiple commands:\n\
  - If independent and can run in parallel, make multiple tool calls in a single message.\n\
  - If dependent and must run sequentially, chain with && in a single call.\n\
  - Use ; only when you need sequential execution but don't care if earlier commands fail.\n\
  - DO NOT use newlines to separate commands.\n\
- For git commands:\n\
  - Prefer creating a new commit rather than amending an existing commit.\n\
  - Before running destructive operations (git reset --hard, git push --force, \
git checkout --), consider safer alternatives.\n\
  - Never skip hooks (--no-verify) or bypass signing unless the user explicitly asks.\n\
- Avoid unnecessary sleep commands:\n\
  - Do not sleep between commands that can run immediately.\n\
  - Do not retry failing commands in a sleep loop — diagnose the root cause.";

fn schema() -> serde_json::Value {
    serde_json::json!({
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

/// Execute a shell command in the session workspace directory.
pub struct ShellTool {
    secret_sink: Arc<dyn SecretUsageSink>,
}

impl ShellTool {
    pub fn new(secret_sink: Arc<dyn SecretUsageSink>) -> Self {
        Self { secret_sink }
    }

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
        ToolId::Bash.as_str()
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
        let command = match args.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return Ok(ToolResult::error("Missing 'command' parameter")),
        };

        let output = ctx.workspace.exec(command, &HashMap::new()).await;

        let secret_ids = ctx.workspace.secret_variable_ids();
        if !secret_ids.is_empty() {
            self.secret_sink.touch_last_used_many(&secret_ids, "");
        }

        slog!(
            info,
            "shell",
            "completed",
            command,
            exit_code = output.exit_code,
            stdout_len = output.stdout.len(),
            stderr_len = output.stderr.len(),
            variable_count = ctx.workspace.build_env().len(),
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
