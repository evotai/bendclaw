use async_trait::async_trait;
use serde_json::json;

use super::claude_agent::ClaudeCodeAgent;
use crate::base::truncate_chars_with_ellipsis;
use crate::base::Result;
use crate::kernel::tools::cli_agent::AgentOptions;
use crate::kernel::tools::cli_agent::AgentProcess;
use crate::kernel::tools::cli_agent::AgentStateKey;
use crate::kernel::tools::cli_agent::CliAgent;
use crate::kernel::tools::cli_agent::CliAgentState;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::Impact;
use crate::kernel::OpType;

static AGENT: ClaudeCodeAgent = ClaudeCodeAgent;

pub struct ClaudeCodeTool;

impl OperationClassifier for ClaudeCodeTool {
    fn op_type(&self) -> OpType {
        OpType::Execute
    }

    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        Some(Impact::High)
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        truncate_chars_with_ellipsis(prompt, 120)
    }
}

#[async_trait]
impl Tool for ClaudeCodeTool {
    fn name(&self) -> &str {
        ToolId::ClaudeCode.as_str()
    }

    fn description(&self) -> &str {
        "Delegate a coding task to Claude Code CLI. Supports multi-turn: subsequent calls \
         resume the same session automatically. Use for complex multi-file edits, refactoring, \
         or code generation."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The coding task to perform"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory (defaults to session workspace)"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let prompt = match args.get("prompt").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return Ok(ToolResult::error("Missing 'prompt' parameter")),
        };

        let cwd = args
            .get("working_dir")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| ctx.workspace.dir().to_str().unwrap_or("."));

        let opts = AgentOptions::default();
        let tool_call_id = ctx.current_tool_call_id().to_string();
        let state_key = AgentStateKey::new(AGENT.agent_type(), cwd);

        let state = ctx.runtime.cli_agent_state.clone();
        let mut guard = state.lock().await;

        if guard.has_followup_process(&state_key) {
            let process = guard.take_followup_process(&state_key).unwrap();
            return self
                .run_followup(process, prompt, ctx, &tool_call_id, &state_key, &mut guard)
                .await;
        }

        if let Some(sid) = guard.get_session_id(&state_key).map(|s| s.to_string()) {
            match AgentProcess::resume(&AGENT, cwd.as_ref(), &sid, prompt, &opts).await {
                Ok(process) => {
                    return self
                        .run_new(process, ctx, &tool_call_id, &state_key, &mut guard)
                        .await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "claude resume failed, starting fresh");
                }
            }
        }

        let process = match AgentProcess::start(&AGENT, cwd.as_ref(), prompt, &opts).await {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::error(format!("{e}"))),
        };

        self.run_new(process, ctx, &tool_call_id, &state_key, &mut guard)
            .await
    }
}

impl ClaudeCodeTool {
    async fn run_new(
        &self,
        mut process: AgentProcess,
        ctx: &ToolContext,
        tool_call_id: &str,
        state_key: &AgentStateKey,
        guard: &mut tokio::sync::MutexGuard<'_, CliAgentState>,
    ) -> Result<ToolResult> {
        match process
            .read_until_result(
                &AGENT,
                ctx.runtime.event_tx.as_ref(),
                tool_call_id,
                &ctx.runtime.cancel,
            )
            .await
        {
            Ok(result) => {
                if let Some(sid) = process.session_id() {
                    guard.set_session_id(state_key.clone(), sid.to_string());
                }
                guard.set_followup_process(state_key.clone(), process);
                Ok(ToolResult::ok(result))
            }
            Err(e) if e.to_string().contains("interrupted") => {
                if let Some(sid) = process.session_id() {
                    guard.set_session_id(state_key.clone(), sid.to_string());
                }
                Ok(ToolResult::error("interrupted"))
            }
            Err(e) => Ok(ToolResult::error(format!("{e}"))),
        }
    }

    async fn run_followup(
        &self,
        mut process: AgentProcess,
        prompt: &str,
        ctx: &ToolContext,
        tool_call_id: &str,
        state_key: &AgentStateKey,
        guard: &mut tokio::sync::MutexGuard<'_, CliAgentState>,
    ) -> Result<ToolResult> {
        if let Err(e) = process.send_followup(&AGENT, prompt).await {
            if let Some(sid) = process.session_id() {
                guard.set_session_id(state_key.clone(), sid.to_string());
            }
            return Ok(ToolResult::error(format!(
                "Follow-up failed: {e}. Use claude_code again to resume."
            )));
        }

        self.run_new(process, ctx, tool_call_id, state_key, guard)
            .await
    }
}
