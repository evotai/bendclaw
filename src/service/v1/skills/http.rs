use axum::extract::Path;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::service::context::RequestContext;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::skills::definition::manifest::SkillManifest;
use crate::skills::definition::skill::Skill;
use crate::skills::definition::skill::SkillFile;
use crate::skills::definition::skill::SkillParameter;
use crate::skills::definition::skill::SkillRequirements;

#[derive(Serialize)]
pub struct SkillResponse {
    pub name: String,
    pub owner_id: String,
    pub version: String,
    pub scope: String,
    pub source: String,
    pub description: String,
    pub content: String,
    pub timeout: u64,
    pub executable: bool,
    pub created_by: Option<String>,
    pub parameters: Vec<SkillParameter>,
    pub files: Vec<SkillFile>,
    pub requires: Option<SkillRequirements>,
    pub manifest: Option<SkillManifest>,
}

fn to_response(s: &Skill, viewer_id: &str) -> SkillResponse {
    SkillResponse {
        name: crate::skills::definition::tool_key::format(s, viewer_id),
        owner_id: s.user_id.clone(),
        version: s.version.clone(),
        scope: s.scope.as_str().to_string(),
        source: s.source.as_str().to_string(),
        description: s.description.clone(),
        content: s.content.clone(),
        timeout: s.timeout,
        executable: s.executable,
        created_by: s.created_by.clone(),
        parameters: s.parameters.clone(),
        files: s.files.clone(),
        requires: s.requires.clone(),
        manifest: s.manifest.clone(),
    }
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
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub parameters: Vec<SkillParameter>,
    #[serde(default)]
    pub files: Vec<SkillFile>,
    #[serde(default)]
    pub requires: Option<SkillRequirements>,
    #[serde(default)]
    pub manifest: Option<SkillManifest>,
}

fn default_version() -> String {
    "0.0.1".to_string()
}

fn default_timeout() -> u64 {
    30
}

pub async fn list_skills(
    State(state): State<AppState>,
    ctx: RequestContext,
) -> Result<Json<Vec<SkillResponse>>> {
    let skills = service::list_skills(&state, &ctx.user_id).await?;
    Ok(Json(
        skills
            .iter()
            .map(|s| to_response(s, &ctx.user_id))
            .collect(),
    ))
}

pub async fn get_skill(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(skill_key): Path<String>,
) -> Result<Json<SkillResponse>> {
    let skill = service::get_skill(&state, &ctx.user_id, &skill_key)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("skill '{skill_key}' not found")))?;
    Ok(Json(to_response(&skill, &ctx.user_id)))
}

pub async fn create_skill(
    State(state): State<AppState>,
    ctx: RequestContext,
    Json(req): Json<CreateSkillRequest>,
) -> Result<Json<SkillResponse>> {
    let skill = service::create_skill(&state, &ctx.user_id, req).await?;
    Ok(Json(to_response(&skill, &ctx.user_id)))
}

pub async fn delete_skill(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(skill_key): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let deleted = service::delete_skill(&state, &ctx.user_id, &skill_key).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}
