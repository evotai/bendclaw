use std::sync::Arc;
use std::time::Duration;

use crate::base::Result;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::session_stream::Stream;

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

impl Runtime {
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
        let normalized = normalize_control_input(input);

        // Cancel commands
        if is_cancel_command(&normalized) {
            let session = self.sessions().get(session_id);
            if let Some(ref s) = session {
                s.cancel_current();
            }
            return Ok(SubmitResult::Control {
                message: "Run cancelled.".to_string(),
            });
        }

        // Status commands
        if is_status_command(&normalized) {
            let session = self.sessions().get(session_id);
            let message = match session {
                Some(ref s) => {
                    let info = s.info();
                    format!("status={} session={}", info.status, info.id)
                }
                None => "No active session.".to_string(),
            };
            return Ok(SubmitResult::Control { message });
        }

        let session = self
            .get_or_create_session(agent_id, session_id, user_id)
            .await?;

        if session.is_running() {
            // Try to inject; if channel full, queue as followup
            if session.inject_message(input) {
                return Ok(SubmitResult::Injected);
            }
            session.queue_followup(input.to_string());
            return Ok(SubmitResult::Queued);
        }

        // Session is idle — start a new run
        let stream = session
            .run(
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
}

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
        .run(&followup, trace_id, None, "", "", false)
        .await
        .ok()?;
    let _ = agent_id; // used for context, session already resolved
    let _ = user_id;
    Some(stream)
}

fn normalize_control_input(input: &str) -> String {
    input.trim().to_lowercase()
}

fn is_cancel_command(normalized: &str) -> bool {
    matches!(
        normalized,
        "stop" | "cancel" | "abort" | "停止" | "取消" | "中止"
    )
}

fn is_status_command(normalized: &str) -> bool {
    matches!(normalized, "status" | "progress" | "状态" | "进度")
}
