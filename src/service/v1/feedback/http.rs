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
use crate::storage::dal::feedback::FeedbackRecord;

#[derive(Serialize)]
pub struct FeedbackResponse {
    pub id: String,
    pub session_id: String,
    pub run_id: String,
    pub rating: i32,
    pub comment: String,
    pub created_at: String,
    pub updated_at: String,
}

fn to_response(r: FeedbackRecord) -> FeedbackResponse {
    FeedbackResponse {
        id: r.id,
        session_id: r.session_id,
        run_id: r.run_id,
        rating: r.rating,
        comment: r.comment,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

#[derive(Deserialize)]
pub struct CreateFeedbackRequest {
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub rating: i32,
    pub comment: Option<String>,
}

pub async fn list_feedback(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<FeedbackResponse>>> {
    let (records, total) = service::list_feedback(&state, &agent_id, &q).await?;
    Ok(Json(Paginated::new(
        records.into_iter().map(to_response).collect(),
        &q,
        total,
    )))
}

pub async fn create_feedback(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateFeedbackRequest>,
) -> Result<Json<FeedbackResponse>> {
    let record = service::create_feedback(&state, &agent_id, req).await?;
    Ok(Json(to_response(record)))
}

pub async fn delete_feedback(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, feedback_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let deleted = service::delete_feedback(&state, &agent_id, &feedback_id).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}
