use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::kernel::run::event::Event;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::writer::tool_op::ToolWriter;

/// Runtime controls injected into tools during execution.
#[derive(Clone)]
pub struct ToolRuntime {
    pub event_tx: Option<mpsc::Sender<Event>>,
    pub cancel: CancellationToken,
    pub tool_call_id: Option<Arc<str>>,
}

/// Per-session identity context passed to tools at execution time.
#[derive(Clone)]
pub struct ToolContext {
    pub user_id: Arc<str>,
    pub session_id: Arc<str>,
    pub agent_id: Arc<str>,
    pub run_id: Arc<str>,
    pub trace_id: Arc<str>,
    pub workspace: Arc<Workspace>,
    /// True when this run was dispatched from a remote node (has parent_run_id).
    /// Prevents nested dispatch — only one level of fanout is allowed.
    pub is_dispatched: bool,
    pub runtime: ToolRuntime,
    pub tool_writer: ToolWriter,
}

impl ToolContext {
    pub fn current_tool_call_id(&self) -> &str {
        match &self.runtime.tool_call_id {
            Some(tool_call_id) => tool_call_id,
            None => &self.run_id,
        }
    }

    /// Send a progress notification to external subscribers (SSE, channels, etc.).
    /// This does NOT inject into LLM messages — it only broadcasts via the event channel.
    pub async fn notify_progress(&self, message: &str) {
        if let Some(ref tx) = self.runtime.event_tx {
            let _ = tx
                .send(Event::Progress {
                    tool_call_id: self.runtime.tool_call_id.as_ref().map(|s| s.to_string()),
                    message: message.to_string(),
                })
                .await;
        }
    }
}
