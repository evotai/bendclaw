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
use crate::service::v1::common::ListQuery;
use crate::service::v1::common::Paginated;

#[derive(Serialize)]
pub struct VariableResponse {
    pub id: String,
    pub key: String,
    pub value: String,
    pub secret: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct CreateVariableRequest {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub secret: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateVariableRequest {
    pub key: Option<String>,
    pub value: Option<String>,
    pub secret: Option<bool>,
}

pub async fn list_variables(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<VariableResponse>>> {
    let (records, total) = service::list_variables(&state, &agent_id, &q).await?;
    Ok(Json(Paginated::new(
        records.into_iter().map(to_response).collect(),
        &q,
        total,
    )))
}

pub async fn create_variable(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateVariableRequest>,
) -> Result<Json<VariableResponse>> {
    let record = service::create_variable(&state, &agent_id, req).await?;
    Ok(Json(to_response(record)))
}

pub async fn get_variable(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, var_id)): Path<(String, String)>,
) -> Result<Json<VariableResponse>> {
    let record = service::get_variable(&state, &agent_id, &var_id).await?;
    Ok(Json(to_response(record)))
}

pub async fn update_variable(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, var_id)): Path<(String, String)>,
    Json(req): Json<UpdateVariableRequest>,
) -> Result<Json<serde_json::Value>> {
    service::update_variable(&state, &agent_id, &var_id, req).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_variable(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, var_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    service::delete_variable(&state, &agent_id, &var_id).await?;
    Ok(Json(serde_json::json!({ "deleted": var_id })))
}

fn to_response(r: crate::storage::dal::variable::VariableRecord) -> VariableResponse {
    VariableResponse {
        id: r.id,
        key: r.key,
        value: if r.secret { "****".to_string() } else { r.value },
        secret: r.secret,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}
