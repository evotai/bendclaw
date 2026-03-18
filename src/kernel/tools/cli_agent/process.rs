use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::Child;
use tokio::process::ChildStderr;
use tokio::process::ChildStdout;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::event::AgentEvent;
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
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
}

impl AgentProcess {
    pub async fn start(
        agent: &dyn CliAgent,
        cwd: &Path,
        prompt: &str,
        opts: &AgentOptions,
    ) -> Result<Self> {
        let initial_prompt = if agent.supports_stdin_followup() {
            ""
        } else {
            prompt
        };

        let mut process = Self::spawn(agent, cwd, initial_prompt, opts).await?;
        if agent.supports_stdin_followup() && !prompt.is_empty() {
            process.send_followup(agent, prompt).await?;
        }
        Ok(process)
    }

    pub async fn spawn(
        agent: &dyn CliAgent,
        cwd: &Path,
        prompt: &str,
        opts: &AgentOptions,
    ) -> Result<Self> {
        let mut cmd = agent.build_command(cwd, prompt, opts);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
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
        let stderr = child.stderr.take().expect("stderr piped");
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(20)));
        let stderr_task =
            spawn_stderr_task(agent.agent_type().to_string(), stderr, stderr_tail.clone());
        Ok(Self {
            child,
            stdout_lines: BufReader::new(stdout).lines(),
            session_id: None,
            agent_type: agent.agent_type().to_string(),
            stderr_tail,
            stderr_task: Some(stderr_task),
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
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to resume {} CLI session {session_id}: {e}",
                agent.agent_type()
            )
        })?;

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(20)));
        let stderr_task =
            spawn_stderr_task(agent.agent_type().to_string(), stderr, stderr_tail.clone());
        Ok(Self {
            child,
            stdout_lines: BufReader::new(stdout).lines(),
            session_id: Some(session_id.to_string()),
            agent_type: agent.agent_type().to_string(),
            stderr_tail,
            stderr_task: Some(stderr_task),
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
                                    tracing::debug!(agent = %self.agent_type, session_id = %sid, "session started");
                                    self.session_id = Some(sid);
                                }
                            }

                            if let Some(result) = agent.parse_result(&msg) {
                                return Ok(result);
                            }

                            for event in agent.parse_events(&msg) {
                                emit_agent_event(event_tx, tool_call_id, event).await;
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

        Err(anyhow::anyhow!(self.drain_stderr_and_build_message().await).into())
    }

    /// Wait for the stderr reader task to finish, then build the error message.
    async fn drain_stderr_and_build_message(&mut self) -> String {
        if let Some(task) = self.stderr_task.take() {
            let _ = task.await;
        }
        self.missing_result_message().await
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

    async fn missing_result_message(&self) -> String {
        let stderr = self.stderr_tail.lock().await;
        if stderr.is_empty() {
            format!("{} CLI exited without a result message", self.agent_type)
        } else {
            format!(
                "{} CLI exited without a result message. stderr: {}",
                self.agent_type,
                stderr.iter().cloned().collect::<Vec<_>>().join(" | ")
            )
        }
    }
}

fn spawn_stderr_task(
    agent_type: String,
    stderr: ChildStderr,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    tracing::warn!(agent = %agent_type, stderr = %line, "cli stderr");
                    let mut tail = stderr_tail.lock().await;
                    if tail.len() >= 20 {
                        tail.pop_front();
                    }
                    tail.push_back(line.to_string());
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(agent = %agent_type, error = %e, "stderr read error");
                    break;
                }
            }
        }
    })
}

pub async fn emit_agent_event(
    event_tx: Option<&mpsc::Sender<Event>>,
    tool_call_id: &str,
    agent_event: AgentEvent,
) {
    if let Some(tx) = event_tx {
        let _ = tx
            .send(Event::ToolUpdate {
                tool_call_id: tool_call_id.to_string(),
                event: agent_event,
            })
            .await;
    }
}
