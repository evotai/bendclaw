use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::Child;
use tokio::process::ChildStdout;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::protocol::CliAgent;
use crate::base::Result;
use crate::kernel::run::event::Event;

#[derive(Debug, Clone, Default)]
pub struct AgentOptions {
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub max_budget_usd: Option<f64>,
}

/// A running CLI agent subprocess.
pub struct AgentProcess {
    child: Child,
    stdout_lines: Lines<BufReader<ChildStdout>>,
    session_id: Option<String>,
    agent_type: String,
}

impl AgentProcess {
    pub async fn spawn(
        agent: &dyn CliAgent,
        cwd: &Path,
        prompt: &str,
        opts: &AgentOptions,
    ) -> Result<Self> {
        let mut cmd = agent.build_command(cwd, prompt, opts);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn {} CLI: {e}. Is '{}' installed and in PATH?",
                agent.agent_type(),
                agent.agent_type()
            )
        })?;

        let stdout = child.stdout.take().expect("stdout piped");
        Ok(Self {
            child,
            stdout_lines: BufReader::new(stdout).lines(),
            session_id: None,
            agent_type: agent.agent_type().to_string(),
        })
    }

    pub async fn resume(
        agent: &dyn CliAgent,
        cwd: &Path,
        session_id: &str,
        prompt: &str,
        opts: &AgentOptions,
    ) -> Result<Self> {
        let mut cmd = agent.build_resume_command(cwd, session_id, prompt, opts);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to resume {} CLI session {session_id}: {e}",
                agent.agent_type()
            )
        })?;

        let stdout = child.stdout.take().expect("stdout piped");
        Ok(Self {
            child,
            stdout_lines: BufReader::new(stdout).lines(),
            session_id: Some(session_id.to_string()),
            agent_type: agent.agent_type().to_string(),
        })
    }

    pub async fn read_until_result(
        &mut self,
        agent: &dyn CliAgent,
        event_tx: Option<&mpsc::Sender<Event>>,
        tool_call_id: &str,
        cancel: &CancellationToken,
    ) -> Result<String> {
        loop {
            tokio::select! {
                line = self.stdout_lines.next_line() => {
                    match line {
                        Err(e) => {
                            tracing::warn!(agent = %self.agent_type, error = %e, "stdout read error");
                            break;
                        }
                        Ok(None) => break,
                        Ok(Some(line)) => {
                            if line.trim().is_empty() {
                                continue;
                            }
                            let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
                                continue;
                            };

                            if self.session_id.is_none() {
                                if let Some(sid) = agent.parse_session_id(&msg) {
                                    tracing::info!(agent = %self.agent_type, session_id = %sid, "session started");
                                    self.session_id = Some(sid);
                                }
                            }

                            if let Some(result) = agent.parse_result(&msg) {
                                return Ok(result);
                            }

                            if let Some(text) = agent.parse_streaming_text(&msg) {
                                if !text.is_empty() {
                                    emit_update(event_tx, tool_call_id, &text).await;
                                }
                            }
                        }
                    }
                }
                _ = cancel.cancelled() => {
                    tracing::info!(agent = %self.agent_type, "cancelled");
                    return Err(anyhow::anyhow!("interrupted").into());
                }
            }
        }

        Err(anyhow::anyhow!("{} CLI exited without a result message", self.agent_type).into())
    }

    pub async fn send_followup(&mut self, agent: &dyn CliAgent, prompt: &str) -> Result<()> {
        let msg = agent.build_stdin_message(prompt).ok_or_else(|| {
            anyhow::anyhow!("{} does not support stdin follow-up", self.agent_type)
        })?;

        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no stdin available"))?;

        stdin.write_all(msg.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn stdout_lines_mut(&mut self) -> &mut Lines<BufReader<ChildStdout>> {
        &mut self.stdout_lines
    }
}

pub async fn emit_update(event_tx: Option<&mpsc::Sender<Event>>, tool_call_id: &str, output: &str) {
    if let Some(tx) = event_tx {
        let _ = tx
            .send(Event::ToolUpdate {
                tool_call_id: tool_call_id.to_string(),
                output: output.to_string(),
            })
            .await;
    }
}
