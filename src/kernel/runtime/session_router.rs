use std::sync::Arc;
use std::time::Duration;

use super::diagnostics;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::runtime::session_stream::Stream;
use crate::types::Result;

#[allow(clippy::large_enum_variant)]
pub enum SubmitResult {
    Started {
        stream: Stream,
        preamble: Option<String>,
    },
    Injected,
    Queued,
    Control {
        message: String,
    },
}

// ── Control command classification ──────────────────────────────────────────

#[derive(Clone, Copy)]
enum ControlCommand {
    Cancel,
    Status,
    NewSession,
    ClearSession,
}

impl ControlCommand {
    fn name(self) -> &'static str {
        match self {
            Self::Cancel => "cancel",
            Self::Status => "status",
            Self::NewSession => "/new",
            Self::ClearSession => "/clear",
        }
    }
}

/// Classify normalized input into a known control command.
/// Returns `None` for regular messages (including unknown slash commands).
fn classify_control_command(normalized: &str) -> Option<ControlCommand> {
    match normalized {
        "stop" | "cancel" | "abort" => Some(ControlCommand::Cancel),
        "status" | "progress" => Some(ControlCommand::Status),
        "/new" => Some(ControlCommand::NewSession),
        "/clear" => Some(ControlCommand::ClearSession),
        _ => None,
    }
}

// ── submit_turn ─────────────────────────────────────────────────────────────

impl Runtime {
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_turn(
        self: &Arc<Self>,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
        input: &str,
        trace_id: &str,
        parent_run_id: Option<&str>,
        parent_trace_id: &str,
        origin_node_id: &str,
        is_remote_dispatch: bool,
    ) -> Result<SubmitResult> {
        let normalized = input.trim().to_lowercase();

        if let Some(command) = classify_control_command(&normalized) {
            diagnostics::log_control_command_classified(diagnostics::ControlCommandInfo {
                agent_id,
                user_id,
                session_id,
                input,
                normalized: &normalized,
                command: command.name(),
                handled: true,
                handler: "runtime.submit_turn",
            });

            return self
                .handle_control_command(command, agent_id, session_id, user_id)
                .await;
        }

        // Log unknown slash commands for diagnostics (not handled).
        if normalized.starts_with('/') {
            diagnostics::log_control_command_classified(diagnostics::ControlCommandInfo {
                agent_id,
                user_id,
                session_id,
                input,
                normalized: &normalized,
                command: "slash_unknown",
                handled: false,
                handler: "none",
            });
        }

        let session = crate::kernel::session::factory::acquire_cloud_session(
            self, agent_id, session_id, user_id,
        )
        .await?;

        if session.is_running() {
            if session.inject_message(input) {
                return Ok(SubmitResult::Injected);
            }
            session.queue_followup(input.to_string());
            return Ok(SubmitResult::Queued);
        }

        let stream = session
            .submit_turn(
                input,
                trace_id,
                parent_run_id,
                parent_trace_id,
                origin_node_id,
                is_remote_dispatch,
            )
            .await?;

        Ok(SubmitResult::Started {
            stream,
            preamble: None,
        })
    }

    async fn handle_control_command(
        self: &Arc<Self>,
        command: ControlCommand,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
    ) -> Result<SubmitResult> {
        let message = match command {
            ControlCommand::Cancel => {
                if let Some(session) = self.sessions().get(session_id) {
                    session.cancel_current();
                }
                "Run cancelled.".to_string()
            }
            ControlCommand::Status => match self.sessions().get(session_id) {
                Some(ref s) => {
                    let info = s.info();
                    format!("status={} session={}", info.status, info.id)
                }
                None => "No active session.".to_string(),
            },
            ControlCommand::ClearSession => {
                if let Some(session) = self.sessions().get(session_id) {
                    session.clear_history();
                }
                "Conversation history cleared.".to_string()
            }
            ControlCommand::NewSession => {
                self.session_lifecycle()
                    .reset_by_id(agent_id, user_id, session_id, "new")
                    .await?;
                "New conversation started.".to_string()
            }
        };
        Ok(SubmitResult::Control { message })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Wait until the session becomes idle, polling at the given interval.
pub async fn wait_until_idle(
    runtime: &Arc<Runtime>,
    session_id: &str,
    poll_interval: Duration,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        if let Some(session) = runtime.sessions().get(session_id) {
            if session.is_idle() {
                return true;
            }
        } else {
            return true;
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Merge a queued followup into a new run if the session is now idle.
pub async fn merge_followup(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    session_id: &str,
    user_id: &str,
    trace_id: &str,
) -> Option<Stream> {
    let session = runtime.sessions().get(session_id)?;
    if !session.is_idle() {
        return None;
    }
    let followup = session.take_followup()?;
    let stream = session
        .submit_turn(&followup, trace_id, None, "", "", false)
        .await
        .ok()?;
    let _ = agent_id;
    let _ = user_id;
    Some(stream)
}
