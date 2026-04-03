use crate::server::state::AppState;
use crate::skills::definition::manifest::CredentialSpec;
use crate::skills::definition::skill::Skill;

pub(super) fn list_hub_skills(state: &AppState) -> Vec<Skill> {
    state.runtime.org().catalog().hub_skills()
}

pub(super) fn hub_status(state: &AppState) -> HubStatus {
    let catalog = state.runtime.org().catalog();
    let hub_config = catalog.hub_config().cloned();
    let last_sync = catalog.hub_last_sync();
    let hub_skill_count = catalog.hub_skills().len();
    HubStatus {
        enabled: hub_config.is_some(),
        repo_url: hub_config
            .as_ref()
            .map(|c| c.repo_url.clone())
            .unwrap_or_default(),
        skill_count: hub_skill_count,
        last_sync_epoch: last_sync
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
    }
}

pub(super) fn skill_credentials(state: &AppState, skill_name: &str) -> Vec<CredentialSpec> {
    let skill = state.runtime.org().catalog().get_hub(skill_name);
    skill
        .and_then(|s| s.manifest)
        .map(|m| m.credentials)
        .unwrap_or_default()
}

pub struct HubStatus {
    pub enabled: bool,
    pub repo_url: String,
    pub skill_count: usize,
    pub last_sync_epoch: Option<u64>,
}
