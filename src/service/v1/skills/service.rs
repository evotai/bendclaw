use super::http::CreateSkillRequest;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillScope;
use crate::kernel::skills::skill::SkillSource;
use crate::service::error::Result;
use crate::service::state::AppState;

pub(super) async fn list_skills(
    state: &AppState,
    agent_id: &str,
    user_id: &str,
) -> Result<Vec<Skill>> {
    Ok(state.runtime.skills().for_agent(agent_id, user_id))
}

pub(super) async fn get_skill(state: &AppState, skill_name: &str) -> Result<Option<Skill>> {
    Ok(state.runtime.skills().get(skill_name))
}

pub(super) async fn create_skill(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    req: CreateSkillRequest,
) -> Result<Skill> {
    let skill = Skill {
        name: req.name,
        version: req.version,
        scope: SkillScope::Agent,
        source: SkillSource::Agent,
        agent_id: Some(agent_id.to_string()),
        user_id: Some(user_id.to_string()),
        description: req.description,
        content: req.content,
        timeout: 30,
        executable: req.executable,
        parameters: Vec::new(),
        files: Vec::new(),
        requires: None,
    };
    state.runtime.create_skill(agent_id, skill.clone()).await?;
    Ok(skill)
}

pub(super) async fn delete_skill(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    skill_name: &str,
) -> Result<String> {
    state
        .runtime
        .delete_skill(agent_id, user_id, skill_name)
        .await?;
    Ok(skill_name.to_string())
}
