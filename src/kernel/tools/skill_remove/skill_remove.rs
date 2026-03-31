//! `remove_skill` tool — lets the agent remove a previously created skill.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::skills::service::SkillService;
use crate::kernel::skills::skill::Skill;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;
pub struct SkillRemoveTool {
    service: Arc<SkillService>,
}

impl SkillRemoveTool {
    pub fn new(service: Arc<SkillService>) -> Self {
        Self { service }
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
        "Remove an owned skill or unsubscribe from a subscribed skill."
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

        let (owner, bare_name) = crate::kernel::skills::tool_key::parse(&name, &ctx.user_id);

        // Validate bare name in all cases
        if let Err(e) = Skill::validate_name(bare_name) {
            return Ok(ToolResult::error(e.message));
        }

        if owner != &*ctx.user_id {
            // Subscribed skill: owner is a user_id, not a skill name — no skill name validation
            if owner.is_empty() || owner.contains("..") {
                return Ok(ToolResult::error("invalid owner in skill key".to_string()));
            }
            // Subscribed skill: unsubscribe
            if let Err(e) = self
                .service
                .unsubscribe(&ctx.user_id, bare_name, owner)
                .await
            {
                return Ok(ToolResult::error(format!(
                    "failed to unsubscribe skill: {e}"
                )));
            }
            Ok(ToolResult::ok(format!("Skill '{name}' unsubscribed")))
        } else {
            // Owned skill: delete
            if let Err(e) = self.service.delete(&ctx.user_id, &name).await {
                return Ok(ToolResult::error(format!("failed to remove skill: {e}")));
            }
            Ok(ToolResult::ok(format!("Skill '{name}' removed")))
        }
    }
}
