use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::service::context::RequestContext;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::storage::SpanRecord;

#[derive(Serialize)]
pub struct TraceResponse {
    pub trace_id: String,
    pub run_id: String,
    pub session_id: String,
    pub name: String,
    pub status: String,
    pub duration_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_cost: f64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub parent_trace_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub origin_node_id: String,
    pub created_at: String,
}

#[derive(Deserialize, Default)]
pub struct TracesQuery {
    #[serde(flatten)]
    pub list: super::super::common::ListQuery,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub user_id: Option<String>,
    pub status: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

#[derive(Serialize)]
pub struct TraceDetailResponse {
    pub trace: TraceResponse,
    pub spans: Vec<SpanRecord>,
}

pub async fn list_traces(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<TracesQuery>,
) -> Result<Json<super::super::common::Paginated<TraceResponse>>> {
    Ok(Json(service::list_traces(&state, &agent_id, q).await?))
}

pub async fn get_trace(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, trace_id)): Path<(String, String)>,
) -> Result<Json<TraceDetailResponse>> {
    Ok(Json(
        service::get_trace(&state, &agent_id, &trace_id).await?,
    ))
}

pub async fn traces_summary(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<crate::storage::AgentTraceSummary>> {
    Ok(Json(service::traces_summary(&state, &agent_id).await?))
}

pub async fn list_spans(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, trace_id)): Path<(String, String)>,
) -> Result<Json<Vec<SpanRecord>>> {
    Ok(Json(
        service::list_spans(&state, &agent_id, &trace_id).await?,
    ))
}

pub async fn list_child_traces(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path((agent_id, trace_id)): Path<(String, String)>,
) -> Result<Json<Vec<TraceResponse>>> {
    Ok(Json(
        service::list_child_traces(&state, &agent_id, &trace_id, &ctx.user_id).await?,
    ))
}
