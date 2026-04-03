use axum::extract::Path;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

use super::service;
use crate::server::context::RequestContext;
use crate::server::error::Result;
use crate::server::state::AppState;

#[derive(Serialize)]
pub struct HubSkillResponse {
    pub name: String,
    pub version: String,
    pub description: String,
    pub executable: bool,
    pub has_credentials: bool,
}

#[derive(Serialize)]
pub struct HubStatusResponse {
    pub enabled: bool,
    pub repo_url: String,
    pub skill_count: usize,
    pub last_sync_epoch: Option<u64>,
}

#[derive(Serialize)]
pub struct CredentialResponse {
    pub env: String,
    pub label: String,
    pub description: Option<String>,
    pub secret: bool,
    pub required: bool,
    pub hint: Option<String>,
    pub placeholder: Option<String>,
    pub setup_url: Option<String>,
    pub validation: Option<String>,
}

pub async fn list_hub_skills(
    State(state): State<AppState>,
    _ctx: RequestContext,
) -> Result<Json<Vec<HubSkillResponse>>> {
    let skills = service::list_hub_skills(&state);
    Ok(Json(
        skills
            .into_iter()
            .map(|s| HubSkillResponse {
                has_credentials: s
                    .manifest
                    .as_ref()
                    .map(|m| !m.credentials.is_empty())
                    .unwrap_or(false),
                name: s.name,
                version: s.version,
                description: s.description,
                executable: s.executable,
            })
            .collect(),
    ))
}

pub async fn hub_status(
    State(state): State<AppState>,
    _ctx: RequestContext,
) -> Result<Json<HubStatusResponse>> {
    let status = service::hub_status(&state);
    Ok(Json(HubStatusResponse {
        enabled: status.enabled,
        repo_url: status.repo_url,
        skill_count: status.skill_count,
        last_sync_epoch: status.last_sync_epoch,
    }))
}

pub async fn skill_credentials(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(skill_name): Path<String>,
) -> Result<Json<Vec<CredentialResponse>>> {
    let creds = service::skill_credentials(&state, &skill_name);
    Ok(Json(
        creds
            .into_iter()
            .map(|c| CredentialResponse {
                env: c.env,
                label: c.label,
                description: c.description,
                secret: c.secret,
                required: c.required,
                hint: c.hint,
                placeholder: c.placeholder,
                setup_url: c.setup_url,
                validation: c.validation,
            })
            .collect(),
    ))
}
