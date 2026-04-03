use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::llm::config::LLMConfig;
use crate::server::context::RequestContext;
use crate::server::error::Result;
use crate::server::state::AppState;
use crate::server::v1::common::ListQuery;
use crate::server::v1::common::Paginated;

#[derive(Serialize)]
pub struct ConfigResponse {
    pub agent_id: String,
    pub system_prompt: String,
    pub identity: String,
    pub soul: String,
    pub token_limit_total: Option<u64>,
    pub token_limit_daily: Option<u64>,
    pub llm_config: Option<LLMConfig>,
}

#[derive(Deserialize)]
pub struct UpdateConfigRequest {
    pub system_prompt: Option<String>,
    pub identity: Option<String>,
    pub soul: Option<String>,
    pub token_limit_total: Option<Option<u64>>,
    pub token_limit_daily: Option<Option<u64>>,
    pub llm_config: Option<Option<LLMConfig>>,
    pub notes: Option<String>,
    pub label: Option<String>,
}

#[derive(Serialize)]
pub struct VersionResponse {
    pub id: String,
    pub version: u32,
    pub label: String,
    pub stage: String,
    pub system_prompt: String,
    pub identity: String,
    pub soul: String,
    pub token_limit_total: Option<u64>,
    pub token_limit_daily: Option<u64>,
    pub llm_config: Option<LLMConfig>,
    pub notes: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct RollbackRequest {
    pub version: u32,
}

pub async fn get_config(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<ConfigResponse>> {
    let response = service::get_config(&state, &agent_id).await?;
    Ok(Json(response))
}

pub async fn update_config(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<serde_json::Value>> {
    let version = service::update_config(&state, &agent_id, req).await?;
    Ok(Json(serde_json::json!({ "ok": true, "version": version })))
}

pub async fn list_versions(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<VersionResponse>>> {
    let response = service::list_versions(&state, &agent_id, q).await?;
    Ok(Json(response))
}

pub async fn get_version(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, version)): Path<(String, u32)>,
) -> Result<Json<VersionResponse>> {
    let response = service::get_version(&state, &agent_id, version).await?;
    Ok(Json(response))
}

pub async fn rollback_config(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<RollbackRequest>,
) -> Result<Json<serde_json::Value>> {
    let version = service::load_version_record(&state, &agent_id, req.version).await?;
    service::rollback_config(&state, &agent_id, version).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "rolled_back_to": req.version }),
    ))
}
