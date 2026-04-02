//! Persist operations — dispatched to a background writer.

use std::sync::Arc;

use super::persist_diagnostics;
use crate::kernel::run::event::Event;
use crate::kernel::run::result::Reason;
use crate::kernel::run::usage::ModelRole;
use crate::kernel::run::usage::UsageEvent;
use crate::kernel::session::store::SessionStore;
use crate::kernel::trace::TraceRecorder;
use crate::kernel::writer::BackgroundWriter;
use crate::observability::log::run_log;
use crate::observability::server_log;
use crate::storage::dal::run::record::RunKind;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run::record::RunStatus;
use crate::storage::dal::run_event::record::RunEventRecord;
use crate::storage::dal::session::repo::SessionRepo;
use crate::storage::dal::session::repo::SessionWrite;

pub enum PersistOp {
    /// Barrier: signals all preceding ops are done. Used by non-stream path
    /// to wait for DB writes before reading back.
    Flush(tokio::sync::oneshot::Sender<()>),
    InitRun {
        storage: Arc<dyn SessionStore>,
        run_id: String,
        session_id: String,
        agent_id: String,
        user_id: String,
        user_message: String,
        parent_run_id: String,
        node_id: String,
    },
    SessionUpsert {
        repo: SessionRepo,
        record: SessionWrite,
    },
    SessionMarkReplaced {
        repo: SessionRepo,
        session_id: String,
        replaced_by_session_id: String,
        reset_reason: String,
    },
    SessionDelete {
        repo: SessionRepo,
        session_id: String,
    },
    RunSuccess {
        storage: Arc<dyn SessionStore>,
        trace: Box<TraceRecorder>,
        run_id: String,
        session_id: String,
        agent_id: Arc<str>,
        user_id: Arc<str>,
        response_text: String,
        error_text: String,
        status: RunStatus,
        metrics_json: String,
        stop_reason: String,
        iterations: u32,
        duration_ms: u64,
        usage: crate::kernel::run::result::Usage,
        provider: String,
        model: String,
        input_price: f64,
        output_price: f64,
        event_records: Vec<RunEventRecord>,
        events: Vec<Event>,
    },
    RunError {
        storage: Arc<dyn SessionStore>,
        trace: TraceRecorder,
        run_id: String,
        session_id: String,
        agent_id: Arc<str>,
        error_text: String,
        duration_ms: u64,
        event_records: Vec<RunEventRecord>,
    },
    RunCancelled {
        storage: Arc<dyn SessionStore>,
        trace: TraceRecorder,
        run_id: String,
        duration_ms: u64,
        event_records: Vec<RunEventRecord>,
    },
    SaveCheckpoint {
        storage: Arc<dyn SessionStore>,
        session_id: String,
        agent_id: String,
        user_id: String,
        summary_text: String,
        through_run_id: String,
    },
}

pub type PersistWriter = BackgroundWriter<PersistOp>;

pub fn spawn_persist_writer() -> PersistWriter {
    BackgroundWriter::spawn("persist", 256, |op| async {
        handle_op(op).await;
        true
    })
}

async fn handle_op(op: PersistOp) {
    match op {
        PersistOp::Flush(tx) => {
            let _ = tx.send(());
        }
        PersistOp::SessionUpsert { repo, record } => {
            if let Err(error) = repo.upsert(record).await {
                persist_diagnostics::log_session_upsert_failed(&error);
            }
        }
        PersistOp::SessionMarkReplaced {
            repo,
            session_id,
            replaced_by_session_id,
            reset_reason,
        } => {
            if let Err(error) = repo
                .mark_replaced(&session_id, &replaced_by_session_id, &reset_reason)
                .await
            {
                persist_diagnostics::log_session_mark_replaced_failed(
                    &session_id,
                    &replaced_by_session_id,
                    &error,
                );
            }
        }
        PersistOp::SessionDelete { repo, session_id } => {
            if let Err(error) = repo.delete_by_id(&session_id).await {
                persist_diagnostics::log_session_delete_failed(&session_id, &error);
            }
        }
        PersistOp::InitRun {
            storage,
            run_id,
            session_id,
            agent_id,
            user_id,
            user_message,
            parent_run_id,
            node_id,
        } => {
            if storage
                .session_load(&session_id)
                .await
                .ok()
                .flatten()
                .is_none()
            {
                if let Err(e) = storage
                    .session_upsert(SessionWrite {
                        session_id: session_id.clone(),
                        agent_id: agent_id.clone(),
                        user_id: user_id.clone(),
                        title: user_message.clone(),
                        base_key: String::new(),
                        replaced_by_session_id: String::new(),
                        reset_reason: String::new(),
                        session_state: serde_json::Value::Null,
                        meta: serde_json::Value::Null,
                    })
                    .await
                {
                    persist_diagnostics::log_run_session_upsert_failed(
                        &run_id,
                        &session_id,
                        &agent_id,
                        &e,
                    );
                }
            }
            if let Err(e) = storage
                .run_insert(&RunRecord {
                    id: run_id.clone(),
                    session_id: session_id.clone(),
                    agent_id: agent_id.clone(),
                    user_id,
                    kind: RunKind::UserTurn.as_str().to_string(),
                    parent_run_id,
                    node_id,
                    status: RunStatus::Running.as_str().to_string(),
                    input: user_message,
                    output: String::new(),
                    error: String::new(),
                    metrics: String::new(),
                    stop_reason: String::new(),
                    checkpoint_through_run_id: String::new(),
                    iterations: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                })
                .await
            {
                persist_diagnostics::log_run_insert_failed(&run_id, &session_id, &agent_id, &e);
            }
        }
        PersistOp::RunSuccess {
            storage,
            trace,
            run_id,
            session_id,
            agent_id,
            user_id: _,
            response_text,
            error_text,
            status,
            metrics_json,
            stop_reason,
            iterations,
            duration_ms,
            usage,
            provider,
            model,
            input_price,
            output_price,
            event_records,
            events: _,
        } => {
            let ctx = server_log::ServerCtx::new(
                &trace.trace_id,
                &run_id,
                &session_id,
                &agent_id,
                iterations,
            );

            // Usage, events, run update — all in parallel
            let usage_fut = record_usage(
                storage.as_ref(),
                &ctx,
                &usage,
                &provider,
                &model,
                input_price,
                output_price,
            );
            let events_fut = persist_event_records(storage.as_ref(), &ctx, &event_records);
            let run_fut = storage.run_update_final(
                &run_id,
                status.clone(),
                &response_text,
                &error_text,
                &metrics_json,
                &stop_reason,
                iterations,
            );

            let (usage_res, events_res, run_res) = tokio::join!(usage_fut, events_fut, run_fut);

            if let Err(e) = usage_res {
                persist_diagnostics::log_usage_failed(&run_id, &session_id, &agent_id, &e);
            }
            if let Err(e) = events_res {
                persist_diagnostics::log_run_events_failed(&run_id, &session_id, &agent_id, &e);
            }
            if let Err(e) = run_res {
                persist_diagnostics::log_run_update_failed(
                    "error",
                    &run_id,
                    &session_id,
                    &agent_id,
                    &e,
                );
            }

            match status {
                RunStatus::Completed | RunStatus::Paused => {
                    trace.complete_trace(
                        duration_ms,
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        0.0,
                    );
                }
                _ => {
                    trace.fail_trace(duration_ms);
                }
            }

            run_log!(info, ctx, "run", "completed",
                msg = format!("─── RUN END {} {} iters {}ms tokens={} (prompt={} comp={}) ───",
                    crate::observability::server_log::short_run_id(&run_id),
                    iterations,
                    duration_ms,
                    usage.total_tokens,
                    usage.prompt_tokens,
                    usage.completion_tokens,
                ),
                model = %model,
                provider = %provider,
                status = %status.as_str(),
                stop_reason = %stop_reason,
                iterations,
                elapsed_ms = duration_ms,
                tokens = usage.total_tokens,
                prompt_tokens = usage.prompt_tokens,
                completion_tokens = usage.completion_tokens,
                ttft_ms = usage.ttft_ms,
                event_count = event_records.len(),
            );
        }
        PersistOp::RunError {
            storage,
            trace,
            run_id,
            session_id,
            agent_id,
            error_text,
            duration_ms,
            event_records,
        } => {
            let ctx =
                server_log::ServerCtx::new(&trace.trace_id, &run_id, &session_id, &agent_id, 0);

            let (events_res, run_res) = tokio::join!(
                persist_event_records(storage.as_ref(), &ctx, &event_records),
                storage.run_update_final(
                    &run_id,
                    RunStatus::Error,
                    "",
                    &error_text,
                    "",
                    Reason::Error.as_str(),
                    0,
                )
            );

            if let Err(e) = events_res {
                persist_diagnostics::log_run_events_failed(&run_id, &session_id, &agent_id, &e);
            }
            if let Err(e) = run_res {
                persist_diagnostics::log_run_update_failed(
                    "warn",
                    &run_id,
                    &session_id,
                    &agent_id,
                    &e,
                );
            }

            trace.fail_trace(duration_ms);

            run_log!(error, ctx, "run", "failed",
                elapsed_ms = duration_ms,
                event_count = event_records.len(),
                error = %error_text,
            );
        }
        PersistOp::RunCancelled {
            storage,
            trace,
            run_id,
            duration_ms,
            event_records,
        } => {
            // Events and status update can be parallel, but we don't have
            // ctx fields here — keep it simple.
            for record in &event_records {
                if let Err(e) = storage
                    .run_events_insert_batch(std::slice::from_ref(record))
                    .await
                {
                    persist_diagnostics::log_cancel_event_failed(&run_id, &e);
                }
            }

            if let Err(e) = storage
                .run_update_status(&run_id, RunStatus::Cancelled)
                .await
            {
                persist_diagnostics::log_cancel_status_failed(&run_id, &e);
            }

            trace.fail_trace(duration_ms);
        }
        PersistOp::SaveCheckpoint {
            storage,
            session_id,
            agent_id,
            user_id,
            summary_text,
            through_run_id,
        } => {
            let run_id = crate::kernel::new_run_id();
            if let Err(e) = storage
                .run_insert(&RunRecord {
                    id: run_id.clone(),
                    session_id: session_id.clone(),
                    agent_id,
                    user_id,
                    kind: RunKind::SessionCheckpoint.as_str().to_string(),
                    parent_run_id: String::new(),
                    node_id: String::new(),
                    status: RunStatus::Completed.as_str().to_string(),
                    input: String::new(),
                    output: summary_text,
                    error: String::new(),
                    metrics: String::new(),
                    stop_reason: String::new(),
                    checkpoint_through_run_id: through_run_id,
                    iterations: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                })
                .await
            {
                persist_diagnostics::log_checkpoint_insert_failed(&run_id, &session_id, &e);
            }
        }
    }
}

async fn record_usage(
    storage: &dyn SessionStore,
    ctx: &server_log::ServerCtx<'_>,
    usage: &crate::kernel::run::result::Usage,
    provider: &str,
    model: &str,
    input_price: f64,
    output_price: f64,
) -> crate::base::Result<()> {
    if usage.total_tokens == 0 {
        return Ok(());
    }
    let cost = (usage.prompt_tokens as f64 * input_price
        + usage.completion_tokens as f64 * output_price)
        / 1_000_000.0;
    let event = UsageEvent {
        agent_id: ctx.agent_id.to_string(),
        user_id: String::new(),
        session_id: ctx.session_id.to_string(),
        run_id: ctx.run_id.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        model_role: ModelRole::Reasoning,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        ttft_ms: usage.ttft_ms,
        cost,
    };
    storage.usage_record(event).await?;
    storage.usage_flush().await
}

async fn persist_event_records(
    storage: &dyn SessionStore,
    ctx: &server_log::ServerCtx<'_>,
    records: &[RunEventRecord],
) -> crate::base::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let result = storage.run_events_insert_batch(records).await;
    match &result {
        Ok(_) => run_log!(
            debug,
            ctx,
            "persist",
            "run_events_saved",
            rows = records.len() as u64,
        ),
        Err(error) => run_log!(error, ctx, "persist", "run_events_failed",
            rows = records.len() as u64,
            error = %error,
        ),
    }
    result
}
