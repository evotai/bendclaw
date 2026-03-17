use std::path::Path;

use tokio::process::Command;

use super::process::AgentOptions;

/// Protocol adapter for a specific CLI agent (claude, codex, etc.).
pub trait CliAgent: Send + Sync {
    fn agent_type(&self) -> &str;

    fn command_name(&self) -> &str {
        self.agent_type()
    }

    fn base_command(&self) -> Command {
        Command::new(self.command_name())
    }

    fn build_command(&self, cwd: &Path, prompt: &str, opts: &AgentOptions) -> Command;

    fn build_resume_command(
        &self,
        cwd: &Path,
        session_id: &str,
        prompt: &str,
        opts: &AgentOptions,
    ) -> Command;

    fn parse_session_id(&self, line: &serde_json::Value) -> Option<String>;
    fn parse_streaming_text(&self, line: &serde_json::Value) -> Option<String>;
    fn parse_result(&self, line: &serde_json::Value) -> Option<String>;

    fn supports_stdin_followup(&self) -> bool {
        false
    }

    fn build_stdin_message(&self, _prompt: &str) -> Option<String> {
        None
    }
}
