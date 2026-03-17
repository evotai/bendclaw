use async_trait::async_trait;
use serde_json::json;

use super::ClaudeCodeAgent;
use super::CodexAgent;
use crate::base::Result;
use crate::kernel::tools::cli_agent::AgentOptions;
use crate::kernel::tools::cli_agent::AgentProcess;
use crate::kernel::tools::cli_agent::CliAgent;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::Impact;
use crate::kernel::OpType;

static CLAUDE_AGENT: ClaudeCodeAgent = ClaudeCodeAgent;
static CODEX_AGENT: CodexAgent = CodexAgent;

#[derive(Debug, Clone)]
enum ReviewTarget {
    Uncommitted,
    Staged,
    Branch(String),
    Commit(String),
}

impl ReviewTarget {
    fn parse(raw: &str) -> Self {
        match raw {
            "" | "uncommitted" => Self::Uncommitted,
            "staged" => Self::Staged,
            t if t.starts_with("branch:") => Self::Branch(t["branch:".len()..].to_string()),
            t if t.starts_with("commit:") => Self::Commit(t["commit:".len()..].to_string()),
            _ => Self::Uncommitted,
        }
    }

    fn git_args(&self) -> Vec<&str> {
        match self {
            Self::Uncommitted => vec!["diff", "HEAD"],
            Self::Staged => vec!["diff", "--cached"],
            Self::Branch(branch) => vec!["diff", branch.as_str()],
            Self::Commit(sha) => vec!["show", sha.as_str(), "--format="],
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Uncommitted => "uncommitted".to_string(),
            Self::Staged => "staged".to_string(),
            Self::Branch(branch) => format!("branch:{branch}"),
            Self::Commit(sha) => format!("commit:{sha}"),
        }
    }
}

async fn resolve_diff(cwd: &str, target: &ReviewTarget) -> Result<String> {
    let output = tokio::process::Command::new("git")
        .args(target.git_args())
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to run git: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("git failed: {stderr}").into());
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    if diff.trim().is_empty() {
        return Err(anyhow::anyhow!("No changes found for target '{}'", target.label()).into());
    }
    Ok(diff)
}

fn build_review_prompt(diff: &str, extra_prompt: &str) -> String {
    if extra_prompt.is_empty() {
        format!(
            "Review the following code changes. List any bugs, security issues, \
             performance problems, or style concerns. Be specific with file names \
             and line references.\n\n```diff\n{diff}\n```"
        )
    } else {
        format!("{extra_prompt}\n\n```diff\n{diff}\n```")
    }
}

pub struct CodeReviewTool;

impl OperationClassifier for CodeReviewTool {
    fn op_type(&self) -> OpType {
        OpType::Execute
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::Low)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let agent = args.get("agent").and_then(|v| v.as_str()).unwrap_or("?");
        let target = ReviewTarget::parse(
            args.get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("uncommitted"),
        );
        format!("review({agent}, {})", target.label())
    }
}

#[async_trait]
impl Tool for CodeReviewTool {
    fn name(&self) -> &str {
        ToolId::CodeReview.as_str()
    }

    fn description(&self) -> &str {
        "Run a code review using a chosen coding agent. Collects the git diff for the \
         specified target and sends it to the agent with review instructions. \
         Use this to have one agent review another agent's work."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "enum": ["claude_code", "codex"],
                    "description": "Which coding agent to use for the review"
                },
                "target": {
                    "type": "string",
                    "description": "What to review: 'uncommitted' (default), 'staged', 'branch:<name>', or 'commit:<sha>'"
                },
                "prompt": {
                    "type": "string",
                    "description": "Additional review instructions (optional)"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory (defaults to session workspace)"
                }
            },
            "required": ["agent"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let agent_name = match args.get("agent").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return Ok(ToolResult::error("Missing 'agent' parameter")),
        };

        let target = ReviewTarget::parse(
            args.get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("uncommitted"),
        );
        let extra_prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let cwd = args
            .get("working_dir")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| ctx.workspace.dir().to_str().unwrap_or("."));

        let diff = match resolve_diff(cwd, &target).await {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::error(format!("{e}"))),
        };
        let review_prompt = build_review_prompt(&diff, extra_prompt);

        let agent: &dyn CliAgent = match agent_name {
            "claude_code" => &CLAUDE_AGENT,
            "codex" => &CODEX_AGENT,
            other => {
                return Ok(ToolResult::error(format!(
                    "Unknown agent '{other}'. Use 'claude_code' or 'codex'."
                )));
            }
        };

        let opts = AgentOptions::default();
        let tool_call_id = ctx.current_tool_call_id().to_string();

        let mut process =
            match AgentProcess::spawn(agent, cwd.as_ref(), &review_prompt, &opts).await {
                Ok(p) => p,
                Err(e) => return Ok(ToolResult::error(format!("{e}"))),
            };

        if agent.supports_stdin_followup() {
            if let Err(e) = process.send_followup(agent, &review_prompt).await {
                return Ok(ToolResult::error(format!(
                    "Failed to send review prompt: {e}"
                )));
            }
        }

        match process
            .read_until_result(
                agent,
                ctx.runtime.event_tx.as_ref(),
                &tool_call_id,
                &ctx.runtime.cancel,
            )
            .await
        {
            Ok(result) => Ok(ToolResult::ok(result)),
            Err(e) if e.to_string().contains("interrupted") => Ok(ToolResult::error("interrupted")),
            Err(e) => Ok(ToolResult::error(format!("{e}"))),
        }
    }
}
