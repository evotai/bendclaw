use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::kernel::run::checkpoint::CompactionCheckpoint;
use crate::kernel::run::compactor::Compactor;
use crate::kernel::run::context::Context;
use crate::kernel::run::dispatcher::ToolDispatcher;
use crate::kernel::run::event::Event;
use crate::kernel::run::run_loop::AbortPolicy;
use crate::kernel::trace::Trace;
use crate::kernel::trace::TraceRecorder;
use crate::kernel::Message;
use crate::observability::audit;
use crate::observability::server_log;

pub(super) const EVENT_CAPACITY: usize = 128;
pub(super) const INBOX_CAPACITY: usize = 16;

pub(crate) struct Engine {
    pub(super) ctx: Context,
    pub(super) compactor: Compactor,
    pub(super) dispatcher: ToolDispatcher,
    pub(super) start_time: Instant,
    pub(super) cancel: CancellationToken,
    pub(super) iteration: Arc<AtomicU32>,
    pub(super) tx: mpsc::Sender<Event>,
    pub(super) trace: Trace,
    pub(super) abort_policy: AbortPolicy,
    pub(super) inbox: mpsc::Receiver<Message>,
    pub(super) loop_span_id: String,
    pub(super) latest_checkpoint: Option<CompactionCheckpoint>,
}

impl Engine {
    /// Create the event channel before building the dispatcher so the `tx`
    /// can be injected into `ToolContext`. Returns `(tx, rx)`.
    pub fn create_channel() -> (mpsc::Sender<Event>, mpsc::Receiver<Event>) {
        mpsc::channel(EVENT_CAPACITY)
    }

    /// Create the inbox channel for message injection. Returns `(tx, rx)`.
    pub fn create_inbox() -> (mpsc::Sender<Message>, mpsc::Receiver<Message>) {
        mpsc::channel(INBOX_CAPACITY)
    }

    /// Build the engine from a pre-created `tx` (from `create_channel`).
    #[allow(clippy::too_many_arguments)]
    pub fn from_tx(
        ctx: Context,
        dispatcher: ToolDispatcher,
        compactor: Compactor,
        cancel: CancellationToken,
        iteration: Arc<AtomicU32>,
        trace_recorder: TraceRecorder,
        tx: mpsc::Sender<Event>,
        inbox: mpsc::Receiver<Message>,
    ) -> Self {
        Self {
            abort_policy: AbortPolicy::new(ctx.max_iterations),
            ctx,
            compactor,
            dispatcher,
            start_time: Instant::now(),
            cancel,
            iteration,
            tx,
            trace: Trace::new(trace_recorder),
            inbox,
            loop_span_id: String::new(),
            latest_checkpoint: None,
        }
    }
    pub(super) fn ops_ctx(&self, turn: u32) -> server_log::ServerCtx<'_> {
        server_log::ServerCtx::new(
            &self.ctx.trace_id,
            &self.ctx.run_id,
            &self.ctx.session_id,
            &self.ctx.agent_id,
            turn,
        )
    }

    pub(super) fn audit_payload(&self, turn: u32) -> serde_json::Map<String, serde_json::Value> {
        audit::base_payload(&self.ops_ctx(turn))
    }

    pub(super) async fn emit_audit(
        &self,
        name: &str,
        payload: serde_json::Map<String, serde_json::Value>,
    ) {
        self.emit(audit::event_from_map(name, payload)).await;
    }

    pub(super) async fn emit(&self, event: Event) {
        let _ = self.tx.send(event).await;
    }
}
