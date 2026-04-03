use std::convert::Infallible;
use std::time::Duration;

use axum::http::StatusCode;
use axum::response::sse::Event as SseEvent;
use axum::response::sse::KeepAlive;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::response::Sse;
use axum::Json;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use super::http::RunEventResponse;
use super::http::RunResponse;
use super::http::RunsQuery;
use super::stream;
use crate::execution::event::Delta;
use crate::execution::event::Event;
use crate::observability::log::slog;
use crate::runtime::SubmitResult;
use crate::server::context::RequestContext;
use crate::server::error::Result;
use crate::server::error::ServiceError;
use crate::server::state::AppState;
use crate::server::v1::common::Paginated;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run::repo::RunRepo;
use crate::storage::dal::run_event::repo::RunEventRepo;

// ── Queries ──────────────────────────────────────────────────────────────

pub async fn list_runs(
    state: &AppState,
    agent_id: &str,
    q: RunsQuery,
) -> Result<Paginated<RunResponse>> {
    let session_id = q.session_id.clone().unwrap_or_default();
    if session_id.trim().is_empty() {
        return Err(ServiceError::BadRequest(
            "session_id is required".to_string(),
        ));
    }

    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = RunRepo::new(pool.clone());
    let total = repo
        .count_for_session(&session_id, q.status.as_deref())
        .await?;
    let rows = repo
        .list_for_session(
            &session_id,
            q.status.as_deref(),
            q.list.order(),
            q.list.limit() as u64,
            q.list.offset() as u64,
        )
        .await?;
    let events_repo = RunEventRepo::new(pool.clone());
    let include_events = q.include_events.unwrap_or(false);

    let mut data = Vec::with_capacity(rows.len());
    for record in rows {
        let events = if include_events {
            Some(load_run_events(&events_repo, &record.id).await?)
        } else {
            None
        };
        data.push(to_response(record, events).map_err(ServiceError::from)?);
    }

    Ok(Paginated::new(data, &q.list, total))
}

pub async fn get_run(state: &AppState, agent_id: &str, run_id: &str) -> Result<RunResponse> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = RunRepo::new(pool.clone());
    let events_repo = RunEventRepo::new(pool);
    let record = repo
        .load(run_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("run '{run_id}' not found")))?;
    let events = load_run_events(&events_repo, run_id).await?;
    to_response(record, Some(events)).map_err(ServiceError::from)
}

pub async fn load_run_record(state: &AppState, agent_id: &str, run_id: &str) -> Result<RunRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = RunRepo::new(pool);
    repo.load(run_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("run '{run_id}' not found")))
}

// ── Commands ─────────────────────────────────────────────────────────────

pub async fn cancel_run(
    state: &AppState,
    agent_id: &str,
    run_id: &str,
) -> Result<serde_json::Value> {
    state.runtime.cancel_run(agent_id, run_id).await?;
    Ok(serde_json::json!({}))
}

pub async fn list_run_events_standalone(
    state: &AppState,
    agent_id: &str,
    run_id: &str,
) -> Result<Vec<RunEventResponse>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let events_repo = RunEventRepo::new(pool);
    load_run_events(&events_repo, run_id).await
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_run(
    state: AppState,
    ctx: RequestContext,
    agent_id: String,
    session_id: String,
    input: String,
    stream_output: bool,
    parent_run_id: Option<String>,
    continue_from_run_id: Option<String>,
    is_remote_dispatch: bool,
) -> Result<Response> {
    slog!(info, "service", "started",
        action = "execute_run",
        trace_id = %ctx.trace_id,
        agent_id = %agent_id,
        session_id = %session_id,
        user_id = %ctx.user_id,
        input_bytes = input.len(),
        stream_output,
        has_parent_run = parent_run_id.is_some(),
        has_continue_from = continue_from_run_id.is_some(),
    );

    // `parent_run_id` may come from a different agent when a run is dispatched
    // across the cluster. The header is internal-only, so preserve lineage even
    // when the parent record does not exist in the current agent database.
    let parent_run_id = if let Some(ref prid) = parent_run_id {
        let pool = state.runtime.databases().agent_pool(&agent_id)?;
        let repo = RunRepo::new(pool);
        match repo.load(prid).await? {
            Some(parent) => {
                if parent.user_id != ctx.user_id {
                    return Err(ServiceError::Forbidden(
                        "parent_run_id belongs to a different user".into(),
                    ));
                }
                parent_run_id
            }
            None => parent_run_id,
        }
    } else {
        None
    };

    state
        .runtime
        .session_lifecycle()
        .ensure_direct(&agent_id, &ctx.user_id, &session_id)
        .await?;

    let submit = state
        .runtime
        .submit_turn(
            &agent_id,
            &session_id,
            &ctx.user_id,
            &input,
            &ctx.trace_id,
            parent_run_id.as_deref(),
            &ctx.parent_trace_id,
            &ctx.origin_node_id,
            is_remote_dispatch,
        )
        .await?;
    let mut run_stream = match submit {
        SubmitResult::Started { stream, .. } => stream,
        SubmitResult::Control { message } => {
            let payload = serde_json::json!({
                "state": "control",
                "message": message,
                "session_id": session_id,
            });
            return Ok((StatusCode::OK, Json(payload)).into_response());
        }
        SubmitResult::Injected => {
            let payload = serde_json::json!({
                "state": "message_injected",
                "session_id": session_id,
            });
            return Ok((StatusCode::ACCEPTED, Json(payload)).into_response());
        }
        SubmitResult::Queued => {
            let payload = serde_json::json!({
                "state": "followup_queued",
                "session_id": session_id,
            });
            return Ok((StatusCode::ACCEPTED, Json(payload)).into_response());
        }
    };
    let run_id = run_stream.run_id().to_string();

    slog!(info, "service", "run_created",
        action = "execute_run",
        trace_id = %ctx.trace_id,
        agent_id = %agent_id,
        session_id = %session_id,
        run_id = %run_id,
        stream_output,
    );

    if !stream_output {
        while run_stream.next().await.is_some() {}
        run_stream.finish().await?;
        // Wait for background persist to complete before reading back.
        state.runtime.flush_persist().await;
        let run = get_run(&state, &agent_id, &run_id).await?;
        return Ok(Json(run).into_response());
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<std::result::Result<SseEvent, Infallible>>(128);
    let spawned_agent_id = agent_id.clone();
    let spawned_session_id = session_id.clone();
    let spawned_run_id = run_id.clone();
    crate::types::spawn_fire_and_forget("run_execution_stream", async move {
        if let Some(from) = continue_from_run_id {
            let payload = stream::base_event_payload(
                &spawned_agent_id,
                &spawned_session_id,
                &spawned_run_id,
                "RunContinued",
            );
            if tx
                .send(Ok(stream::encode_sse("RunContinued", payload)))
                .await
                .is_err()
            {
                return;
            }
            slog!(info, "service", "run_continued",
                action = "execute_run",
                agent_id = %spawned_agent_id,
                session_id = %spawned_session_id,
                run_id = %spawned_run_id,
                from_run_id = %from,
            );
        }

        while let Some(event) = run_stream.next().await {
            if let Some(sse_event) = stream::map_event_to_sse(
                &spawned_agent_id,
                &spawned_session_id,
                &spawned_run_id,
                &event,
            ) {
                if tx.send(Ok(sse_event)).await.is_err() {
                    slog!(warn, "service", "sse_client_closed",
                        action = "execute_run",
                        agent_id = %spawned_agent_id,
                        session_id = %spawned_session_id,
                        run_id = %spawned_run_id,
                    );
                    break;
                }
            }
        }

        if let Err(err) = run_stream.finish().await {
            let mut payload = stream::base_event_payload(
                &spawned_agent_id,
                &spawned_session_id,
                &spawned_run_id,
                "RunError",
            );
            payload["content"] = serde_json::Value::String(err.to_string());
            let _ = tx.send(Ok(stream::encode_sse("RunError", payload))).await;
            slog!(error, "service", "stream_failed",
                action = "execute_run",
                agent_id = %spawned_agent_id,
                session_id = %spawned_session_id,
                run_id = %spawned_run_id,
                error = %err,
            );
        }
    });

    let stream = ReceiverStream::new(rx);
    let sse = Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keepalive"),
    );
    Ok(sse.into_response())
}

// ── Helpers ──────────────────────────────────────────────────────────────

async fn load_run_events(repo: &RunEventRepo, run_id: &str) -> Result<Vec<RunEventResponse>> {
    let rows = repo.list_by_run(run_id, 5000).await?;
    rows.into_iter()
        .filter_map(|r| {
            let event: Event = match serde_json::from_str(&r.payload) {
                Ok(e) => e,
                Err(_) => return None, // skip unparseable records
            };
            if should_skip_event(&event) {
                return None;
            }
            let payload = match r.payload_json() {
                Ok(p) => p,
                Err(_) => return None,
            };
            Some(Ok(RunEventResponse {
                seq: r.seq,
                event: r.event,
                payload,
                created_at: r.created_at,
            }))
        })
        .collect()
}

pub(super) fn should_skip_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Aborted { .. }
            | Event::TurnStart { .. }
            | Event::TurnEnd { .. }
            | Event::AppData(_)
            | Event::StreamDelta(
                Delta::ToolCallStart { .. }
                    | Delta::ToolCallDelta { .. }
                    | Delta::ToolCallEnd { .. }
                    | Delta::Usage(_)
            )
    )
}

fn to_response(
    record: RunRecord,
    events: Option<Vec<RunEventResponse>>,
) -> std::result::Result<RunResponse, crate::types::ErrorCode> {
    let metrics = record.metrics_json()?;
    Ok(RunResponse {
        id: record.id,
        session_id: record.session_id,
        status: record.status,
        input: record.input,
        output: record.output,
        error: record.error,
        metrics,
        stop_reason: record.stop_reason,
        iterations: record.iterations,
        parent_run_id: record.parent_run_id,
        node_id: record.node_id,
        created_at: record.created_at,
        updated_at: record.updated_at,
        events,
    })
}
