use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::server::context::RequestContext;
use crate::server::error::Result;
use crate::server::state::AppState;
use crate::server::v1::common::ListQuery;
use crate::variables::store::Variable;

#[derive(Serialize)]
pub struct VariableResponse {
    pub id: String,
    pub key: String,
    pub value: String,
    pub secret: bool,
    pub revoked: bool,
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct CreateVariableRequest {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub secret: Option<bool>,
    #[serde(default)]
    pub revoked: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateVariableRequest {
    pub key: Option<String>,
    pub value: Option<String>,
    pub secret: Option<bool>,
    pub revoked: Option<bool>,
}

pub async fn list_variables(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(_agent_id): Path<String>,
    Query(_q): Query<ListQuery>,
) -> Result<Json<Vec<VariableResponse>>> {
    let records = service::list_variables(&state, &ctx.user_id).await?;
    Ok(Json(records.into_iter().map(to_response).collect()))
}

pub async fn create_variable(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(_agent_id): Path<String>,
    Json(req): Json<CreateVariableRequest>,
) -> Result<Json<VariableResponse>> {
    let record = service::create_variable(&state, &ctx.user_id, req).await?;
    Ok(Json(to_response(record)))
}

pub async fn get_variable(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path((_agent_id, var_id)): Path<(String, String)>,
) -> Result<Json<VariableResponse>> {
    let record = service::get_variable(&state, &ctx.user_id, &var_id).await?;
    Ok(Json(to_response(record)))
}

pub async fn update_variable(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path((_agent_id, var_id)): Path<(String, String)>,
    Json(req): Json<UpdateVariableRequest>,
) -> Result<Json<serde_json::Value>> {
    service::update_variable(&state, &ctx.user_id, &var_id, req).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_variable(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path((_agent_id, var_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    service::delete_variable(&state, &ctx.user_id, &var_id).await?;
    Ok(Json(serde_json::json!({ "deleted": var_id })))
}

fn to_response(r: Variable) -> VariableResponse {
    VariableResponse {
        id: r.id,
        key: r.key,
        value: if r.secret {
            "****".to_string()
        } else {
            r.value
        },
        secret: r.secret,
        revoked: r.revoked,
        last_used_at: r.last_used_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}
