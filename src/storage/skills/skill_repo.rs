use async_trait::async_trait;

use crate::types::entities::Skill;
use crate::types::Result;

#[async_trait]
pub trait SkillRepo: Send + Sync {
    async fn get_skill(
        &self,
        user_id: &str,
        agent_id: &str,
        skill_id: &str,
    ) -> Result<Option<Skill>>;
    async fn save_skill(&self, skill: &Skill) -> Result<()>;
    async fn delete_skill(&self, user_id: &str, agent_id: &str, skill_id: &str) -> Result<()>;
    async fn list_skills(&self, user_id: &str, agent_id: &str) -> Result<Vec<Skill>>;
}
