use std::convert::Infallible;
use std::time::Duration;

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
use crate::service::context::RequestContext;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::Paginated;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run::repo::RunRepo;
use crate::storage::dal::run_event::repo::RunEventRepo;
use crate::storage::sql;

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
    let sid = sql::escape(&session_id);
    let mut cond = format!("session_id = '{sid}'");
    if let Some(ref status) = q.status {
        cond.push_str(&format!(" AND status = '{}'", sql::escape(status)));
    }
    let total = count_u64(&pool, &format!("SELECT COUNT(*) FROM runs WHERE {cond}")).await;
    let rows = pool
        .query_all(&format!(
            "SELECT id, session_id, agent_id, user_id, parent_run_id, status, input, output, error, metrics, stop_reason, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) \
             FROM runs WHERE {cond} ORDER BY created_at {} LIMIT {} OFFSET {}",
            q.list.order(),
            q.list.limit(),
            q.list.offset()
        ))
        .await?;
    let events_repo = RunEventRepo::new(pool.clone());
    let include_events = q.include_events.unwrap_or(false);

    let mut data = Vec::with_capacity(rows.len());
    for row in rows {
        let record = row_to_run_record(&row);
        let events = if include_events {
            Some(load_run_events(&events_repo, &record.id).await?)
        } else {
            None
        };
        data.push(to_response(record, events));
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
    Ok(to_response(record, Some(events)))
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
    state
        .runtime
        .cancel_run(agent_id, run_id)
        .await
        .map_err(ServiceError::from)?;
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
) -> Result<Response> {
    tracing::info!(
        log_kind = "server_log",
        stage = "service",
        action = "execute_run",
        status = "started",
        trace_id = %ctx.trace_id,
        agent_id = %agent_id,
        session_id = %session_id,
        user_id = %ctx.user_id,
        input_bytes = input.len(),
        stream_output,
        has_parent_run = parent_run_id.is_some(),
        has_continue_from = continue_from_run_id.is_some(),
        "service command"
    );

    let session = state
        .runtime
        .get_or_create_session(&agent_id, &session_id, &ctx.user_id)
        .await
        .map_err(ServiceError::from)?;

    let mut run_stream = session
        .run(&input, &ctx.trace_id, parent_run_id.as_deref())
        .await
        .map_err(ServiceError::from)?;
    let run_id = run_stream.run_id().to_string();

    tracing::info!(
        log_kind = "server_log",
        stage = "service",
        action = "execute_run",
        status = "run_created",
        trace_id = %ctx.trace_id,
        agent_id = %agent_id,
        session_id = %session_id,
        run_id = %run_id,
        stream_output,
        "service command"
    );

    if !stream_output {
        while run_stream.next().await.is_some() {}
        run_stream.finish().await.map_err(ServiceError::from)?;
        let run = get_run(&state, &agent_id, &run_id).await?;
        return Ok(Json(run).into_response());
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<std::result::Result<SseEvent, Infallible>>(128);
    let spawned_agent_id = agent_id.clone();
    let spawned_session_id = session_id.clone();
    let spawned_run_id = run_id.clone();
    tokio::spawn(async move {
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
            tracing::info!(
                log_kind = "server_log",
                stage = "service",
                action = "execute_run",
                status = "continued",
                agent_id = %spawned_agent_id,
                session_id = %spawned_session_id,
                run_id = %spawned_run_id,
                from_run_id = %from,
                "service command"
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
                    tracing::warn!(
                        log_kind = "server_log",
                        stage = "service",
                        action = "execute_run",
                        status = "sse_client_closed",
                        agent_id = %spawned_agent_id,
                        session_id = %spawned_session_id,
                        run_id = %spawned_run_id,
                        "service command"
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
            tracing::error!(
                log_kind = "server_log",
                stage = "service",
                action = "execute_run",
                status = "stream_failed",
                agent_id = %spawned_agent_id,
                session_id = %spawned_session_id,
                run_id = %spawned_run_id,
                error = %err,
                "service command"
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
    Ok(rows
        .into_iter()
        .map(|r| RunEventResponse {
            seq: r.seq,
            event: r.event,
            payload: serde_json::from_str(&r.payload).unwrap_or(serde_json::Value::Null),
            created_at: r.created_at,
        })
        .collect())
}

fn to_response(record: RunRecord, events: Option<Vec<RunEventResponse>>) -> RunResponse {
    let metrics = if record.metrics.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&record.metrics).unwrap_or(serde_json::Value::Null)
    };
    RunResponse {
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
        created_at: record.created_at,
        updated_at: record.updated_at,
        events,
    }
}

fn row_to_run_record(row: &serde_json::Value) -> RunRecord {
    RunRecord {
        id: sql::col(row, 0),
        session_id: sql::col(row, 1),
        agent_id: sql::col(row, 2),
        user_id: sql::col(row, 3),
        parent_run_id: sql::col(row, 4),
        status: sql::col(row, 5),
        input: sql::col(row, 6),
        output: sql::col(row, 7),
        error: sql::col(row, 8),
        metrics: sql::col(row, 9),
        stop_reason: sql::col(row, 10),
        iterations: sql::col(row, 11).parse().unwrap_or(0),
        created_at: sql::col(row, 12),
        updated_at: sql::col(row, 13),
    }
}
