//! `remove_skill` tool — lets the agent remove a previously created skill.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::skills::remote::repository::DatabendSkillRepositoryFactory;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::store::SkillStore;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;

pub struct SkillRemoveTool {
    store_factory: Arc<DatabendSkillRepositoryFactory>,
    store: Arc<SkillStore>,
}

impl SkillRemoveTool {
    pub fn new(store_factory: Arc<DatabendSkillRepositoryFactory>, store: Arc<SkillStore>) -> Self {
        Self {
            store_factory,
            store,
        }
    }
}

impl OperationClassifier for SkillRemoveTool {
    fn op_type(&self) -> OpType {
        OpType::SkillRun
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for SkillRemoveTool {
    fn name(&self) -> &str {
        ToolId::SkillRemove.as_str()
    }

    fn description(&self) -> &str {
        "Remove a previously created skill."
    }

    fn hint(&self) -> &str {
        "remove a skill"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the skill to remove"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if let Err(e) = Skill::validate_name(&name) {
            return Ok(ToolResult::error(e.message));
        }

        let store = match self.store_factory.for_agent(&ctx.agent_id) {
            Ok(s) => s,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "failed to access agent store: {e}"
                )))
            }
        };

        if let Err(e) = store.remove(&name, Some(&ctx.agent_id)).await {
            return Ok(ToolResult::error(format!("failed to remove skill: {e}")));
        }

        self.store.evict(&name, &ctx.agent_id);

        tracing::info!(stage = "skill_remove", status = "completed", skill = %name, "skill_remove completed");
        Ok(ToolResult::ok(format!("Skill '{name}' removed")))
    }
}
