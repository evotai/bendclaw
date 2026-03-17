use std::path::Path;

use serde_json::json;
use tokio::process::Command;

use crate::kernel::tools::cli_agent::AgentOptions;
use crate::kernel::tools::cli_agent::CliAgent;

pub struct ClaudeCodeAgent;

impl CliAgent for ClaudeCodeAgent {
    fn agent_type(&self) -> &str {
        "claude"
    }

    fn build_command(&self, cwd: &Path, _prompt: &str, opts: &AgentOptions) -> Command {
        let mut cmd = self.base_command();
        cmd.current_dir(cwd);
        cmd.args([
            "-p",
            "--output-format",
            "stream-json",
            "--input-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "bypassPermissions",
        ]);
        if let Some(model) = &opts.model {
            cmd.args(["--model", model]);
        }
        if let Some(sp) = &opts.system_prompt {
            cmd.args(["--append-system-prompt", sp]);
        }
        if let Some(budget) = opts.max_budget_usd {
            cmd.args(["--max-budget-usd", &budget.to_string()]);
        }
        cmd.stdin(std::process::Stdio::piped());
        cmd
    }

    fn build_resume_command(
        &self,
        cwd: &Path,
        session_id: &str,
        _prompt: &str,
        opts: &AgentOptions,
    ) -> Command {
        let mut cmd = self.base_command();
        cmd.current_dir(cwd);
        cmd.args([
            "-p",
            "--output-format",
            "stream-json",
            "--input-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "bypassPermissions",
            "--resume",
            session_id,
        ]);
        if let Some(model) = &opts.model {
            cmd.args(["--model", model]);
        }
        cmd.stdin(std::process::Stdio::piped());
        cmd
    }

    fn parse_session_id(&self, line: &serde_json::Value) -> Option<String> {
        if line.get("type")?.as_str()? == "system"
            && line.get("subtype").and_then(|s| s.as_str()) == Some("init")
        {
            return line.get("session_id")?.as_str().map(|s| s.to_string());
        }
        None
    }

    fn parse_streaming_text(&self, line: &serde_json::Value) -> Option<String> {
        match line.get("type")?.as_str()? {
            "assistant" => {
                let blocks = line.get("message")?.get("content")?.as_array()?;
                let mut parts = Vec::new();
                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                parts.push(text.to_string());
                            }
                        }
                        Some("tool_use") => {
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                            parts.push(format!("[tool_use:{name}]"));
                        }
                        _ => {}
                    }
                }
                let text = parts.join("\n");
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            "system" => {
                let subtype = line.get("subtype").and_then(|s| s.as_str())?;
                Some(format!("[system:{subtype}]"))
            }
            _ => None,
        }
    }

    fn parse_result(&self, line: &serde_json::Value) -> Option<String> {
        if line.get("type")?.as_str()? != "result" {
            return None;
        }
        match line.get("result") {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(v) => Some(v.to_string()),
            None => Some(line.to_string()),
        }
    }

    fn supports_stdin_followup(&self) -> bool {
        true
    }

    fn build_stdin_message(&self, prompt: &str) -> Option<String> {
        let msg = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{"type": "text", "text": prompt}]
            }
        });
        Some(format!("{}\n", msg))
    }
}
