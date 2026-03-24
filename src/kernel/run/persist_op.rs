//! Persist operations — dispatched to a background writer.

use std::sync::Arc;

use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::event::Event;
use crate::kernel::run::result::Reason;
use crate::kernel::run::usage::ModelRole;
use crate::kernel::run::usage::UsageEvent;
use crate::kernel::trace::TraceRecorder;
use crate::kernel::writer::BackgroundWriter;
use crate::observability::log::run_log;
use crate::observability::log::slog;
use crate::observability::server_log;
use crate::storage::dal::run::record::RunKind;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run::record::RunStatus;
use crate::storage::dal::run_event::record::RunEventRecord;

pub enum PersistOp {
    /// Barrier: signals all preceding ops are done. Used by non-stream path
    /// to wait for DB writes before reading back.
    Flush(tokio::sync::oneshot::Sender<()>),
    InitRun {
        storage: Arc<AgentStore>,
        run_id: String,
        session_id: String,
        agent_id: String,
        user_id: String,
        user_message: String,
        parent_run_id: String,
        node_id: String,
    },
    RunSuccess {
        storage: Arc<AgentStore>,
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
        event_records: Vec<RunEventRecord>,
        events: Vec<Event>,
    },
    RunError {
        storage: Arc<AgentStore>,
        trace: TraceRecorder,
        run_id: String,
        session_id: String,
        agent_id: Arc<str>,
        error_text: String,
        duration_ms: u64,
        event_records: Vec<RunEventRecord>,
    },
    RunCancelled {
        storage: Arc<AgentStore>,
        trace: TraceRecorder,
        run_id: String,
        duration_ms: u64,
        event_records: Vec<RunEventRecord>,
    },
    SaveCheckpoint {
        storage: Arc<AgentStore>,
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
                    .session_upsert(&session_id, &agent_id, &user_id, Some(&user_message), None)
                    .await
                {
                    slog!(warn, "persist", "session_upsert_failed",
                        run_id = %run_id, session_id = %session_id, agent_id = %agent_id, error = %e,
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
                slog!(warn, "persist", "run_insert_failed",
                    run_id = %run_id, session_id = %session_id, agent_id = %agent_id, error = %e,
                );
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
            let usage_fut = record_usage(&storage, &ctx, &usage, &provider, &model);
            let events_fut = persist_event_records(&storage, &ctx, &event_records);
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
                slog!(warn, "persist", "usage_failed",
                    run_id = %run_id, session_id = %session_id, agent_id = %*agent_id, error = %e,
                );
            }
            if let Err(e) = events_res {
                slog!(error, "persist", "run_events_failed",
                    run_id = %run_id, session_id = %session_id, agent_id = %*agent_id, error = %e,
                );
            }
            if let Err(e) = run_res {
                slog!(error, "persist", "run_update_failed",
                    run_id = %run_id, session_id = %session_id, agent_id = %*agent_id, error = %e,
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
                persist_event_records(&storage, &ctx, &event_records),
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
                slog!(error, "persist", "run_events_failed",
                    run_id = %run_id, session_id = %session_id, agent_id = %*agent_id, error = %e,
                );
            }
            if let Err(e) = run_res {
                slog!(warn, "persist", "run_update_failed",
                    run_id = %run_id, session_id = %session_id, agent_id = %*agent_id, error = %e,
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
                    slog!(error, "persist", "cancel_event_failed",
                        run_id = %run_id, error = %e,
                    );
                }
            }

            if let Err(e) = storage
                .run_update_status(&run_id, RunStatus::Cancelled)
                .await
            {
                slog!(warn, "persist", "cancel_status_failed",
                    run_id = %run_id, error = %e,
                );
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
                slog!(warn, "persist", "checkpoint_insert_failed",
                    run_id = %run_id, session_id = %session_id, error = %e,
                );
            }
        }
    }
}

async fn record_usage(
    storage: &AgentStore,
    ctx: &server_log::ServerCtx<'_>,
    usage: &crate::kernel::run::result::Usage,
    provider: &str,
    model: &str,
) -> crate::base::Result<()> {
    if usage.total_tokens == 0 {
        return Ok(());
    }
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
        cost: 0.0,
    };
    storage.usage_record(event).await?;
    storage.usage_flush().await
}

async fn persist_event_records(
    storage: &AgentStore,
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
