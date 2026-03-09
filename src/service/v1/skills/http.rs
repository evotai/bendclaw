use axum::extract::Path;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::kernel::skills::skill::Skill;
use crate::service::context::RequestContext;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;

#[derive(Serialize)]
pub struct SkillResponse {
    pub name: String,
    pub version: String,
    pub scope: String,
    pub source: String,
    pub description: String,
    pub executable: bool,
}

fn to_response(s: &Skill) -> SkillResponse {
    SkillResponse {
        name: s.name.clone(),
        version: s.version.clone(),
        scope: s.scope.as_str().to_string(),
        source: s.source.as_str().to_string(),
        description: s.description.clone(),
        executable: s.executable,
    }
}

#[derive(Serialize)]
pub struct SkillDetailResponse {
    pub name: String,
    pub version: String,
    pub scope: String,
    pub source: String,
    pub description: String,
    pub content: String,
    pub executable: bool,
}

#[derive(Deserialize)]
pub struct CreateSkillRequest {
    pub name: String,
    pub description: String,
    pub content: String,
    #[serde(default)]
    pub executable: bool,
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "0.0.1".to_string()
}

pub async fn list_skills(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<Vec<SkillResponse>>> {
    let skills = service::list_skills(&state, &agent_id, &ctx.user_id).await?;
    Ok(Json(skills.iter().map(to_response).collect()))
}

pub async fn get_skill(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((_agent_id, skill_name)): Path<(String, String)>,
) -> Result<Json<SkillDetailResponse>> {
    let skill = service::get_skill(&state, &skill_name)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("skill '{skill_name}' not found")))?;
    Ok(Json(SkillDetailResponse {
        name: skill.name.clone(),
        version: skill.version.clone(),
        scope: skill.scope.as_str().to_string(),
        source: skill.source.as_str().to_string(),
        description: skill.description.clone(),
        content: skill.content.clone(),
        executable: skill.executable,
    }))
}

pub async fn create_skill(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateSkillRequest>,
) -> Result<Json<SkillResponse>> {
    let skill = service::create_skill(&state, &ctx.user_id, &agent_id, req).await?;
    Ok(Json(to_response(&skill)))
}

pub async fn delete_skill(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path((agent_id, skill_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let deleted = service::delete_skill(&state, &ctx.user_id, &agent_id, &skill_name).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}
