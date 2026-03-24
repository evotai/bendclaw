//! Run persistence: assembles PersistOps and sends to background writer.

use std::sync::Arc;
use std::time::Instant;

use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::recall::RecallStore;
use crate::kernel::run::event::Event;
use crate::kernel::run::persist_op::PersistOp;
use crate::kernel::run::persist_op::PersistWriter;
use crate::kernel::run::result::Reason;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::trace::TraceRecorder;
use crate::observability::audit;
use crate::observability::log::run_log;
use crate::observability::log::slog;
use crate::observability::server_log;
use crate::storage::dal::run::record::RunMetrics;
use crate::storage::dal::run::record::RunStatus;
use crate::storage::dal::run_event::record::RunEventRecord;

pub struct TurnPersister {
    storage: Arc<AgentStore>,
    trace: TraceRecorder,
    agent_id: Arc<str>,
    session_id: String,
    run_id: String,
    user_id: Arc<str>,
    start: Instant,
    recall: Option<Arc<RecallStore>>,
    writer: PersistWriter,
}

impl TurnPersister {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage: Arc<AgentStore>,
        trace: TraceRecorder,
        agent_id: Arc<str>,
        session_id: impl Into<String>,
        run_id: impl Into<String>,
        user_id: Arc<str>,
        start: Instant,
        recall: Option<Arc<RecallStore>>,
        writer: PersistWriter,
    ) -> Self {
        Self {
            storage,
            trace,
            agent_id,
            session_id: session_id.into(),
            run_id: run_id.into(),
            user_id,
            start,
            recall,
            writer,
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Assemble a success op and send to background writer. Returns response
    /// text immediately — DB writes happen asynchronously.
    pub fn persist_success(
        self,
        result: AgentResult,
        provider: &str,
        model: &str,
        events: &[Event],
    ) -> Result<String> {
        let response_text = result.text();
        let duration_ms = self.start.elapsed().as_millis() as u64;

        let metrics = RunMetrics {
            prompt_tokens: result.usage.prompt_tokens,
            completion_tokens: result.usage.completion_tokens,
            reasoning_tokens: result.usage.reasoning_tokens,
            total_tokens: result.usage.total_tokens,
            cache_read_tokens: result.usage.cache_read_tokens,
            cache_write_tokens: result.usage.cache_write_tokens,
            ttft_ms: result.usage.ttft_ms,
            duration_ms,
            cost: 0.0,
        };
        let metrics_json = serde_json::to_string(&metrics)?;
        let status = status_from_reason(&result.stop_reason);
        let error_text = if matches!(status, RunStatus::Error) {
            response_text.clone()
        } else {
            String::new()
        };

        let mut all_events = events.to_vec();
        let mut payload = audit::base_payload(&self.ops_ctx(result.iterations));
        payload.insert(
            "user_id".to_string(),
            serde_json::json!(self.user_id.to_string()),
        );
        payload.insert("status".to_string(), serde_json::json!(status.as_str()));
        payload.insert("provider".to_string(), serde_json::json!(provider));
        payload.insert("model".to_string(), serde_json::json!(model));
        payload.insert(
            "iterations".to_string(),
            serde_json::json!(result.iterations),
        );
        payload.insert(
            "stop_reason".to_string(),
            serde_json::json!(result.stop_reason.as_str()),
        );
        payload.insert(
            "output".to_string(),
            serde_json::json!(response_text.clone()),
        );
        payload.insert("error".to_string(), serde_json::json!(error_text.clone()));
        payload.insert("usage".to_string(), serde_json::json!(result.usage.clone()));
        payload.insert("metrics".to_string(), serde_json::json!(metrics.clone()));
        payload.insert(
            "content".to_string(),
            serde_json::json!(result.content.clone()),
        );
        payload.insert(
            "messages".to_string(),
            serde_json::json!(result.messages.clone()),
        );
        all_events.push(audit::event_from_map("run.completed", payload));

        let event_records = self.build_event_records(&all_events);

        self.writer.send(PersistOp::RunSuccess {
            storage: self.storage,
            trace: Box::new(self.trace),
            recall: self.recall,
            run_id: self.run_id,
            session_id: self.session_id,
            agent_id: self.agent_id,
            user_id: self.user_id,
            response_text: response_text.clone(),
            error_text,
            status,
            metrics_json,
            stop_reason: result.stop_reason.as_str().to_string(),
            iterations: result.iterations,
            duration_ms,
            usage: result.usage,
            provider: provider.to_string(),
            model: model.to_string(),
            event_records,
            events: events.to_vec(),
        });

        Ok(response_text)
    }

    /// Assemble an error op and send to background writer.
    pub fn persist_error(self, error: &crate::base::ErrorCode, events: &[Event]) {
        let duration_ms = self.start.elapsed().as_millis() as u64;
        let error_text = format!("{error}");

        slog!(error, "run", "failed",
            agent_id = %self.agent_id,
            session_id = %self.session_id,
            run_id = %self.run_id,
            elapsed_ms = duration_ms,
            error = %error_text,
        );

        let mut all_events = events.to_vec();
        let mut payload = audit::base_payload(&self.ops_ctx(0));
        payload.insert(
            "user_id".to_string(),
            serde_json::json!(self.user_id.to_string()),
        );
        payload.insert(
            "status".to_string(),
            serde_json::json!(RunStatus::Error.as_str()),
        );
        payload.insert("error".to_string(), serde_json::json!(error_text.clone()));
        all_events.push(audit::event_from_map("run.failed", payload));

        let event_records = self.build_event_records(&all_events);

        self.writer.send(PersistOp::RunError {
            storage: self.storage,
            trace: self.trace,
            run_id: self.run_id,
            session_id: self.session_id,
            agent_id: self.agent_id,
            error_text,
            duration_ms,
            event_records,
        });
    }

    /// Assemble a cancellation op and send to background writer.
    pub fn persist_cancelled(self, events: &[Event]) {
        let duration_ms = self.start.elapsed().as_millis() as u64;

        let mut all_events = events.to_vec();
        let mut payload = audit::base_payload(&self.ops_ctx(0));
        payload.insert(
            "user_id".to_string(),
            serde_json::json!(self.user_id.to_string()),
        );
        payload.insert(
            "status".to_string(),
            serde_json::json!(RunStatus::Cancelled.as_str()),
        );
        payload.insert("error".to_string(), serde_json::json!("cancelled"));
        all_events.push(audit::event_from_map("run.cancelled", payload));

        let event_records = self.build_event_records(&all_events);

        run_log!(
            warn,
            self.ops_ctx(0),
            "run",
            "cancelled",
            elapsed_ms = duration_ms,
            event_count = all_events.len(),
        );

        self.writer.send(PersistOp::RunCancelled {
            storage: self.storage,
            trace: self.trace,
            run_id: self.run_id,
            duration_ms,
            event_records,
        });
    }

    fn ops_ctx(&self, turn: u32) -> server_log::ServerCtx<'_> {
        server_log::ServerCtx::new(
            &self.trace.trace_id,
            &self.run_id,
            &self.session_id,
            &self.agent_id,
            turn,
        )
    }

    fn build_event_records(&self, events: &[Event]) -> Vec<RunEventRecord> {
        events
            .iter()
            .enumerate()
            .filter_map(|(idx, event)| {
                Some(RunEventRecord {
                    id: crate::kernel::new_id(),
                    run_id: self.run_id.clone(),
                    session_id: self.session_id.clone(),
                    agent_id: self.agent_id.to_string(),
                    user_id: self.user_id.to_string(),
                    seq: (idx + 1) as u32,
                    event: event.name(),
                    payload: serde_json::to_string(event).ok()?,
                    created_at: String::new(),
                })
            })
            .collect()
    }
}

pub fn status_from_reason(reason: &Reason) -> RunStatus {
    match reason {
        Reason::EndTurn => RunStatus::Completed,
        Reason::MaxIterations | Reason::Timeout => RunStatus::Paused,
        Reason::Aborted => RunStatus::Cancelled,
        Reason::Error => RunStatus::Error,
    }
}
