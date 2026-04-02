//! NoopSkillExecutor — returns error for all skill calls. For bendclaw-local.

use async_trait::async_trait;

use super::skill_executor::SkillExecutor;
use super::skill_executor::SkillOutput;
use crate::base::ErrorCode;
use crate::base::Result;

pub struct NoopSkillExecutor;

#[async_trait]
impl SkillExecutor for NoopSkillExecutor {
    async fn execute(&self, skill_name: &str, _args: &[String]) -> Result<SkillOutput> {
        Err(ErrorCode::internal(format!(
            "skill '{skill_name}' is not available in local mode"
        )))
    }
}
