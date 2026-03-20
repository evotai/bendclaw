use std::path::Path;
use std::sync::LazyLock;

use serde_json::json;
use tokio::process::Command;

use crate::kernel::tools::cli_agent::AgentEvent;
use crate::kernel::tools::cli_agent::AgentOptions;
use crate::kernel::tools::cli_agent::CliAgent;

/// Custom command parsed from `BENDCLAW_CLAUDE_COMMAND` env var.
/// Supports multi-word commands like `ccr claude`.
/// Falls back to `claude` when the env var is unset or empty.
static CUSTOM_COMMAND: LazyLock<Vec<String>> = LazyLock::new(|| {
    std::env::var("BENDCLAW_CLAUDE_COMMAND")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
});

pub struct ClaudeCodeAgent;

impl CliAgent for ClaudeCodeAgent {
    fn agent_type(&self) -> &str {
        "claude"
    }

    fn command_name(&self) -> &str {
        if CUSTOM_COMMAND.is_empty() {
            "claude"
        } else {
            &CUSTOM_COMMAND[0]
        }
    }

    fn base_command(&self) -> Command {
        if CUSTOM_COMMAND.is_empty() {
            return Command::new("claude");
        }
        let mut cmd = Command::new(&CUSTOM_COMMAND[0]);
        if CUSTOM_COMMAND.len() > 1 {
            cmd.args(&CUSTOM_COMMAND[1..]);
        }
        cmd
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

    fn parse_events(&self, line: &serde_json::Value) -> Vec<AgentEvent> {
        let Some(msg_type) = line.get("type").and_then(|t| t.as_str()) else {
            return vec![];
        };

        match msg_type {
            "assistant" => self.parse_assistant(line),
            "system" => self.parse_system(line),
            "user" => self.parse_user_tool_results(line),
            _ => vec![],
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

impl ClaudeCodeAgent {
    fn parse_assistant(&self, line: &serde_json::Value) -> Vec<AgentEvent> {
        let Some(blocks) = line
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        else {
            return vec![];
        };

        let mut events = Vec::new();
        for block in blocks {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            events.push(AgentEvent::Text {
                                content: text.to_string(),
                            });
                        }
                    }
                }
                "thinking" => {
                    if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            events.push(AgentEvent::Thinking {
                                content: text.to_string(),
                            });
                        }
                    }
                }
                "tool_use" => {
                    let name = block
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown");
                    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let input = block
                        .get("input")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    events.push(AgentEvent::ToolUse {
                        tool_name: name.to_string(),
                        tool_use_id: id.to_string(),
                        input,
                    });
                }
                _ => {}
            }
        }
        events
    }

    fn parse_system(&self, line: &serde_json::Value) -> Vec<AgentEvent> {
        let subtype = line
            .get("subtype")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");
        let mut metadata = serde_json::Map::new();
        if let Some(session_id) = line.get("session_id") {
            metadata.insert("session_id".to_string(), session_id.clone());
        }
        if let Some(model) = line.get("model") {
            metadata.insert("model".to_string(), model.clone());
        }
        if let Some(cwd) = line.get("cwd") {
            metadata.insert("cwd".to_string(), cwd.clone());
        }
        vec![AgentEvent::System {
            subtype: subtype.to_string(),
            metadata: serde_json::Value::Object(metadata),
        }]
    }

    fn parse_user_tool_results(&self, line: &serde_json::Value) -> Vec<AgentEvent> {
        let Some(blocks) = line
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        else {
            return vec![];
        };

        let mut events = Vec::new();
        for block in blocks {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            let tool_use_id = block
                .get("tool_use_id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            let is_error = block
                .get("is_error")
                .and_then(|e| e.as_bool())
                .unwrap_or(false);
            let output = extract_tool_result_text(block);
            events.push(AgentEvent::ToolResult {
                tool_use_id,
                success: !is_error,
                output,
            });
        }
        events
    }
}

fn extract_tool_result_text(block: &serde_json::Value) -> String {
    if let Some(content) = block.get("content") {
        if let Some(s) = content.as_str() {
            return s.to_string();
        }
        if let Some(arr) = content.as_array() {
            let mut parts = Vec::new();
            for item in arr {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    parts.push(text.to_string());
                }
            }
            return parts.join("\n");
        }
    }
    String::new()
}
