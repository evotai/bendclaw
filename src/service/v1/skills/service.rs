use super::http::CreateSkillRequest;
use crate::kernel::skills::definition::skill::Skill;
use crate::kernel::skills::definition::skill::SkillScope;
use crate::kernel::skills::definition::skill::SkillSource;
use crate::service::error::Result;
use crate::service::state::AppState;

pub(super) async fn list_skills(state: &AppState, user_id: &str) -> Result<Vec<Skill>> {
    Ok(state.runtime.org().catalog().visible_skills(user_id))
}

pub(super) async fn get_skill(
    state: &AppState,
    user_id: &str,
    skill_key: &str,
) -> Result<Option<Skill>> {
    Ok(state.runtime.org().catalog().resolve(user_id, skill_key))
}

pub(super) async fn create_skill(
    state: &AppState,
    user_id: &str,
    req: CreateSkillRequest,
) -> Result<Skill> {
    let skill = Skill {
        name: req.name,
        version: req.version,
        scope: SkillScope::Shared,
        source: SkillSource::Agent,
        user_id: user_id.to_string(),
        created_by: Some(user_id.to_string()),
        last_used_by: None,
        description: req.description,
        content: req.content,
        timeout: req.timeout,
        executable: req.executable,
        parameters: req.parameters,
        files: req.files,
        requires: req.requires,
        manifest: req.manifest,
    };
    skill.validate()?;
    state.runtime.create_skill(user_id, skill.clone()).await?;
    Ok(skill)
}

pub(super) async fn delete_skill(
    state: &AppState,
    user_id: &str,
    skill_key: &str,
) -> Result<String> {
    let (owner, bare_name) = crate::kernel::skills::definition::tool_key::parse(skill_key, user_id);
    if owner != user_id {
        // Subscribed skill: unsubscribe instead of delete
        state
            .runtime
            .org()
            .manager()
            .unsubscribe(user_id, bare_name, owner)
            .await?;
    } else {
        // Owned skill: delete
        state.runtime.delete_skill(user_id, bare_name).await?;
    }
    Ok(skill_key.to_string())
}
