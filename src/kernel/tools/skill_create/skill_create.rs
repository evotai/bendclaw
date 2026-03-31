//! `create_skill` tool — lets the agent author new executable skills at runtime.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::skills::service::SkillService;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillFile;
use crate::kernel::skills::skill::SkillScope;
use crate::kernel::skills::skill::SkillSource;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;
pub struct SkillCreateTool {
    service: Arc<SkillService>,
}

impl SkillCreateTool {
    pub fn new(service: Arc<SkillService>) -> Self {
        Self { service }
    }
}

impl OperationClassifier for SkillCreateTool {
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
impl Tool for SkillCreateTool {
    fn name(&self) -> &str {
        ToolId::SkillCreate.as_str()
    }

    fn description(&self) -> &str {
        "Create a new executable skill with a SKILL.md and an entry-point script."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name (lowercase alphanumeric + dash, 2-64 chars)"
                },
                "description": {
                    "type": "string",
                    "description": "Human-readable description of what this skill does"
                },
                "version": {
                    "type": "string",
                    "description": "Semver version (default: 0.1.0)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Execution timeout in seconds (default: 30)"
                },
                "content": {
                    "type": "string",
                    "description": "SKILL.md body: parameters section + usage instructions"
                },
                "script_name": {
                    "type": "string",
                    "description": "Entry script filename: 'run.py' or 'run.sh'"
                },
                "script_body": {
                    "type": "string",
                    "description": "Script source code"
                }
            },
            "required": ["name", "description", "content", "script_name", "script_body"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let name = arg_str(&args, "name");
        let description = arg_str(&args, "description");
        let version = arg_str_or(&args, "version", "0.1.0");
        let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);
        let content = arg_str(&args, "content");
        let script_name = arg_str(&args, "script_name");
        let script_body = arg_str(&args, "script_body");

        if let Err(e) = Skill::validate_name(&name) {
            return Ok(ToolResult::error(e.message));
        }

        let file_path = format!("scripts/{script_name}");
        if let Err(e) = Skill::validate_file_path(&file_path) {
            return Ok(ToolResult::error(e.message));
        }

        let files = vec![SkillFile {
            path: file_path,
            body: script_body,
        }];

        if let Err(e) = Skill::validate_size(&content, &files) {
            return Ok(ToolResult::error(e.message));
        }

        let skill = Skill {
            name: name.clone(),
            version: version.clone(),
            description,
            scope: SkillScope::Shared,
            source: SkillSource::Agent,
            user_id: ctx.user_id.to_string(),
            created_by: Some(ctx.user_id.to_string()),
            last_used_by: None,
            timeout,
            executable: true,
            parameters: crate::kernel::skills::fs::parse_parameters_section(&content),
            content,
            files,
            requires: None,
            manifest: None,
        };

        if let Err(e) = self.service.create(&ctx.user_id, skill).await {
            return Ok(ToolResult::error(format!("failed to save skill: {e}")));
        }

        Ok(ToolResult::ok(format!(
            "Skill '{name}' created (v{version})"
        )))
    }
}

fn arg_str(args: &serde_json::Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn arg_str_or(args: &serde_json::Value, key: &str, default: &str) -> String {
    let v = arg_str(args, key);
    if v.is_empty() {
        default.to_string()
    } else {
        v
    }
}
