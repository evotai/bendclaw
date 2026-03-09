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

#[derive(Serialize)]
pub struct UsageSummaryResponse {
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_reasoning_tokens: u64,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub record_count: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_write_tokens: u64,
}

#[derive(Serialize)]
pub struct DailyUsageResponse {
    pub date: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost: f64,
    pub requests: u64,
}

#[derive(Deserialize, Default)]
pub struct DailyQuery {
    pub days: Option<u32>,
}

pub async fn usage_summary(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<UsageSummaryResponse>> {
    Ok(Json(service::usage_summary(&state, &agent_id).await?))
}

pub async fn usage_daily(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<DailyQuery>,
) -> Result<Json<Vec<DailyUsageResponse>>> {
    Ok(Json(service::usage_daily(&state, &agent_id, q).await?))
}

pub async fn global_usage_summary(
    State(state): State<AppState>,
    _ctx: RequestContext,
) -> Result<Json<UsageSummaryResponse>> {
    Ok(Json(service::global_usage_summary(&state).await?))
}
