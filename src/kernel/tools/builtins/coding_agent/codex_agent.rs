use std::path::Path;

use tokio::process::Command;

use crate::kernel::tools::cli_agent::AgentOptions;
use crate::kernel::tools::cli_agent::CliAgent;

pub struct CodexAgent;

impl CliAgent for CodexAgent {
    fn agent_type(&self) -> &str {
        "codex"
    }

    fn build_command(&self, cwd: &Path, prompt: &str, opts: &AgentOptions) -> Command {
        let mut cmd = self.base_command();
        cmd.current_dir(cwd);
        cmd.args([
            "exec",
            "--json",
            "--dangerously-bypass-approvals-and-sandbox",
        ]);
        if let Some(model) = &opts.model {
            cmd.args(["--model", model]);
        }
        cmd.arg(prompt);
        cmd
    }

    fn build_resume_command(
        &self,
        cwd: &Path,
        session_id: &str,
        prompt: &str,
        opts: &AgentOptions,
    ) -> Command {
        let mut cmd = self.base_command();
        cmd.current_dir(cwd);
        cmd.args([
            "exec",
            "resume",
            "--json",
            "--dangerously-bypass-approvals-and-sandbox",
            session_id,
        ]);
        if let Some(model) = &opts.model {
            cmd.args(["--model", model]);
        }
        if !prompt.is_empty() {
            cmd.arg(prompt);
        }
        cmd
    }

    fn parse_session_id(&self, line: &serde_json::Value) -> Option<String> {
        if line.get("type")?.as_str()? == "thread.started" {
            return line.get("thread_id")?.as_str().map(|s| s.to_string());
        }
        None
    }

    fn parse_streaming_text(&self, line: &serde_json::Value) -> Option<String> {
        if line.get("type")?.as_str()? != "item.completed" {
            return None;
        }
        let item = line.get("item")?;
        match item.get("type")?.as_str()? {
            "agent_message" => item.get("text")?.as_str().map(|s| s.to_string()),
            "command_execution" => {
                let cmd = item.get("command").and_then(|c| c.as_str()).unwrap_or("?");
                let exit = item.get("exit_code").and_then(|c| c.as_i64());
                Some(format!("[exec:{cmd} exit:{exit:?}]"))
            }
            _ => None,
        }
    }

    fn parse_result(&self, line: &serde_json::Value) -> Option<String> {
        if line.get("type")?.as_str()? != "turn.completed" {
            return None;
        }
        Some(line.to_string())
    }
}
