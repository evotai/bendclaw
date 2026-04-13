use std::sync::Arc;

use super::set::truncate_str;
use super::set::SkillSet;
use crate::types::*;

pub struct SkillTool {
    skills: Arc<SkillSet>,
    description: String,
}

impl SkillTool {
    const MAX_DESC_CHARS: usize = 250;

    pub fn new(skills: Arc<SkillSet>) -> Self {
        let mut desc = String::from(
            "Activate a skill by name. Skills provide specialized capabilities and domain knowledge.\n\n\
             When the user's request matches an available skill, this is a BLOCKING REQUIREMENT: \
             invoke this tool BEFORE generating any other response. \
             NEVER mention a skill without actually calling this tool.\n\n\
             Available skills:\n",
        );
        for skill in skills.specs() {
            let truncated = truncate_str(&skill.description, Self::MAX_DESC_CHARS);
            desc.push_str(&format!("- {}: {}\n", skill.name, truncated));
        }
        Self {
            skills,
            description: desc,
        }
    }
}

fn normalize_name(name: &str) -> &str {
    name.strip_prefix('/').unwrap_or(name)
}

#[async_trait::async_trait]
impl AgentTool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn label(&self) -> &str {
        "Skill"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "Name of the skill to activate"
                }
            },
            "required": ["skill_name"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let name = normalize_name(params.get("skill_name").and_then(|v| v.as_str())?);
        match self.skills.find(name) {
            Some(skill) => Some(format!(
                "loading skill: {} ({})",
                name,
                skill.base_dir.display()
            )),
            None => Some(format!("loading skill: {name}")),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let raw_name = params
            .get("skill_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing 'skill_name' parameter".into()))?;

        let name = normalize_name(raw_name);

        let skill = self.skills.find(name).ok_or_else(|| {
            let available: Vec<&str> = self
                .skills
                .specs()
                .iter()
                .map(|s| s.name.as_str())
                .collect();
            ToolError::Failed(format!(
                "Unknown skill: {name}. Available skills: {}",
                available.join(", ")
            ))
        })?;

        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!(
                    "Activated skill: {name}\n\
                     All relative paths in this skill (e.g. scripts/...) \
                     must be resolved against: {base_dir}\n\n\
                     Follow the instructions below.\n\n\
                     ---\n{instructions}",
                    base_dir = skill.base_dir.display(),
                    instructions = skill.instructions,
                ),
            }],
            details: serde_json::json!({ "skill": name }),
            retention: Retention::CurrentRun,
        })
    }
}
