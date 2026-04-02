//! Run persistence: assembles PersistOps and sends to background writer.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;

use super::persist_op::PersistOp;
use super::persist_op::PersistWriter;
use super::persister_diagnostics;
use crate::kernel::run::event::Event;
use crate::kernel::run::result::Reason;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::session::backend::sink::RunPersister;
use crate::kernel::session::store::SessionStore;
use crate::kernel::trace::TraceRecorder;
use crate::llm::provider::LLMProvider;
use crate::observability::audit;
use crate::observability::log::run_log;
use crate::observability::server_log;
use crate::storage::dal::run::record::RunMetrics;
use crate::storage::dal::run::record::RunStatus;
use crate::storage::dal::run_event::record::RunEventRecord;
use crate::types::ErrorCode;

struct Inner {
    storage: Arc<dyn SessionStore>,
    trace: TraceRecorder,
    agent_id: Arc<str>,
    session_id: String,
    user_id: Arc<str>,
    start: Instant,
    writer: PersistWriter,
    llm: Arc<dyn LLMProvider>,
}

/// Persists run lifecycle events to the background writer.
/// Implements RunSink so Stream can hold it as Arc<dyn RunSink>.
pub struct TurnPersister {
    run_id: String,
    inner: Mutex<Option<Inner>>,
}

impl TurnPersister {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage: Arc<dyn SessionStore>,
        trace: TraceRecorder,
        agent_id: Arc<str>,
        session_id: impl Into<String>,
        run_id: impl Into<String>,
        user_id: Arc<str>,
        start: Instant,
        writer: PersistWriter,
        llm: Arc<dyn LLMProvider>,
    ) -> Self {
        let run_id = run_id.into();
        Self {
            run_id,
            inner: Mutex::new(Some(Inner {
                storage,
                trace,
                agent_id,
                session_id: session_id.into(),
                user_id,
                start,
                writer,
                llm,
            })),
        }
    }
}

impl RunPersister for TurnPersister {
    fn persist_success(&self, result: AgentResult, provider: &str, model: &str, events: &[Event]) {
        let i = match self.inner.lock().take() {
            Some(i) => i,
            None => return,
        };
        do_persist_success(&self.run_id, i, result, provider, model, events);
    }

    fn persist_error(&self, error: &ErrorCode, events: &[Event]) {
        let i = match self.inner.lock().take() {
            Some(i) => i,
            None => return,
        };
        do_persist_error(&self.run_id, i, error, events);
    }

    fn persist_cancelled(&self, events: &[Event]) {
        let i = match self.inner.lock().take() {
            Some(i) => i,
            None => return,
        };
        do_persist_cancelled(&self.run_id, i, events);
    }
}

// ─── Implementation helpers (consume Inner) ─────────────────────────────────

fn ops_ctx<'a>(
    trace: &'a TraceRecorder,
    run_id: &'a str,
    session_id: &'a str,
    agent_id: &'a str,
    turn: u32,
) -> server_log::ServerCtx<'a> {
    server_log::ServerCtx::new(&trace.trace_id, run_id, session_id, agent_id, turn)
}

fn build_event_records(
    run_id: &str,
    session_id: &str,
    agent_id: &str,
    user_id: &str,
    events: &[Event],
) -> Vec<RunEventRecord> {
    events
        .iter()
        .enumerate()
        .filter_map(|(idx, event)| {
            Some(RunEventRecord {
                id: crate::kernel::new_id(),
                run_id: run_id.to_string(),
                session_id: session_id.to_string(),
                agent_id: agent_id.to_string(),
                user_id: user_id.to_string(),
                seq: (idx + 1) as u32,
                event: event.name(),
                payload: serde_json::to_string(event).ok()?,
                created_at: String::new(),
            })
        })
        .collect()
}

fn do_persist_success(
    run_id: &str,
    i: Inner,
    result: AgentResult,
    provider: &str,
    model: &str,
    events: &[Event],
) {
    let response_text = result.text();
    let checkpoint = result.checkpoint.clone();
    let duration_ms = i.start.elapsed().as_millis() as u64;
    let ctx = ops_ctx(
        &i.trace,
        run_id,
        &i.session_id,
        &i.agent_id,
        result.iterations,
    );

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
    let metrics_json = serde_json::to_string(&metrics).unwrap_or_default();
    let status = status_from_reason(&result.stop_reason);
    let error_text = if matches!(status, RunStatus::Error) {
        response_text.clone()
    } else {
        String::new()
    };

    run_log!(info, ctx, "persist", "final_output",
        msg = "persist final output prepared",
        status = %status.as_str(),
        stop_reason = %result.stop_reason.as_str(),
        output_preview = %server_log::preview_text(&response_text),
        output_bytes = response_text.len() as u64,
        content_blocks = result.content.len(),
        message_count = result.messages.len(),
    );

    let mut all_events = events.to_vec();
    let mut payload = audit::base_payload(&ctx);
    payload.insert(
        "user_id".to_string(),
        serde_json::json!(i.user_id.to_string()),
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

    let event_records =
        build_event_records(run_id, &i.session_id, &i.agent_id, &i.user_id, &all_events);

    let (input_price, output_price) = i.llm.pricing(model).unwrap_or((3.0, 15.0));

    i.writer.send(PersistOp::RunSuccess {
        storage: i.storage.clone(),
        trace: Box::new(i.trace),
        run_id: run_id.to_string(),
        session_id: i.session_id.clone(),
        agent_id: i.agent_id.clone(),
        user_id: i.user_id.clone(),
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
        input_price,
        output_price,
        event_records,
        events: events.to_vec(),
    });

    if let Some(checkpoint) = checkpoint {
        i.writer.send(PersistOp::SaveCheckpoint {
            storage: i.storage,
            session_id: i.session_id,
            agent_id: i.agent_id.to_string(),
            user_id: i.user_id.to_string(),
            summary_text: checkpoint.summary_text,
            through_run_id: checkpoint.through_run_id,
        });
    }
}

fn do_persist_error(run_id: &str, i: Inner, error: &ErrorCode, events: &[Event]) {
    let duration_ms = i.start.elapsed().as_millis() as u64;
    let error_text = format!("{error}");

    persister_diagnostics::log_run_failed(
        &i.agent_id,
        &i.session_id,
        run_id,
        duration_ms,
        &error_text,
    );

    let mut all_events = events.to_vec();
    let mut payload =
        audit::base_payload(&ops_ctx(&i.trace, run_id, &i.session_id, &i.agent_id, 0));
    payload.insert(
        "user_id".to_string(),
        serde_json::json!(i.user_id.to_string()),
    );
    payload.insert(
        "status".to_string(),
        serde_json::json!(RunStatus::Error.as_str()),
    );
    payload.insert("error".to_string(), serde_json::json!(error_text.clone()));
    all_events.push(audit::event_from_map("run.failed", payload));

    let event_records =
        build_event_records(run_id, &i.session_id, &i.agent_id, &i.user_id, &all_events);

    i.writer.send(PersistOp::RunError {
        storage: i.storage,
        trace: i.trace,
        run_id: run_id.to_string(),
        session_id: i.session_id,
        agent_id: i.agent_id,
        error_text,
        duration_ms,
        event_records,
    });
}

fn do_persist_cancelled(run_id: &str, i: Inner, events: &[Event]) {
    let duration_ms = i.start.elapsed().as_millis() as u64;
    let ctx = ops_ctx(&i.trace, run_id, &i.session_id, &i.agent_id, 0);

    let mut all_events = events.to_vec();
    let mut payload = audit::base_payload(&ctx);
    payload.insert(
        "user_id".to_string(),
        serde_json::json!(i.user_id.to_string()),
    );
    payload.insert(
        "status".to_string(),
        serde_json::json!(RunStatus::Cancelled.as_str()),
    );
    payload.insert("error".to_string(), serde_json::json!("cancelled"));
    all_events.push(audit::event_from_map("run.cancelled", payload));

    let event_records =
        build_event_records(run_id, &i.session_id, &i.agent_id, &i.user_id, &all_events);

    run_log!(
        warn,
        ctx,
        "run",
        "cancelled",
        elapsed_ms = duration_ms,
        event_count = all_events.len(),
    );

    i.writer.send(PersistOp::RunCancelled {
        storage: i.storage,
        trace: i.trace,
        run_id: run_id.to_string(),
        duration_ms,
        event_records,
    });
}

pub fn status_from_reason(reason: &Reason) -> RunStatus {
    match reason {
        Reason::EndTurn => RunStatus::Completed,
        Reason::MaxIterations | Reason::Timeout => RunStatus::Paused,
        Reason::Aborted => RunStatus::Cancelled,
        Reason::Error => RunStatus::Error,
    }
}
