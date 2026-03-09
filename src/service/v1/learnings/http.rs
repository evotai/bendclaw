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
use crate::storage::LearningRecord;

#[derive(Serialize)]
pub struct LearningResponse {
    pub id: String,
    pub agent_id: String,
    pub user_id: String,
    pub session_id: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

fn to_response(r: LearningRecord) -> LearningResponse {
    let tags = if r.tags.is_empty() {
        Vec::new()
    } else {
        r.tags.split(',').map(|s| s.trim().to_string()).collect()
    };
    LearningResponse {
        id: r.id,
        agent_id: r.agent_id,
        user_id: r.user_id,
        session_id: r.session_id,
        title: r.title,
        content: r.content,
        tags,
        source: r.source,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

#[derive(Deserialize)]
pub struct CreateLearningRequest {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub session_id: String,
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "manual".to_string()
}

#[derive(Deserialize)]
pub struct SearchLearningRequest {
    pub query: String,
    pub limit: Option<u32>,
}

pub async fn create_learning(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateLearningRequest>,
) -> Result<Json<LearningResponse>> {
    let record = service::create_learning(&state, &ctx.user_id, &agent_id, req).await?;
    Ok(Json(to_response(record)))
}

pub async fn list_learnings(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<LearningResponse>>> {
    let (records, total) = service::list_learnings(&state, &agent_id, &q).await?;
    Ok(Json(Paginated::new(
        records.into_iter().map(to_response).collect(),
        &q,
        total,
    )))
}

pub async fn search_learnings(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<SearchLearningRequest>,
) -> Result<Json<Vec<LearningResponse>>> {
    let records = service::search_learnings(&state, &agent_id, &req.query, req.limit).await?;
    Ok(Json(records.into_iter().map(to_response).collect()))
}

pub async fn delete_learning(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, learning_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let deleted = service::delete_learning(&state, &agent_id, &learning_id).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}
