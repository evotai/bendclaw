use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::skills::sanitizer::sanitize_skill_content;
use crate::kernel::skills::service::SkillService;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;
use crate::observability::log::slog;

/// Maximum skill content size returned to the LLM (64 KiB).
/// Matches ironclaw's `MAX_PROMPT_FILE_SIZE`.
const MAX_SKILL_CONTENT_BYTES: usize = 64 * 1024;

/// In-process tool that reads skill documentation.
pub struct SkillReadTool {
    service: Arc<SkillService>,
}

impl SkillReadTool {
    pub fn new(service: Arc<SkillService>) -> Self {
        Self { service }
    }
}

impl OperationClassifier for SkillReadTool {
    fn op_type(&self) -> OpType {
        OpType::SkillRun
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for SkillReadTool {
    fn name(&self) -> &str {
        ToolId::SkillRead.as_str()
    }

    fn description(&self) -> &str {
        "Read skill documentation and reference files."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Skill name or path (e.g. 'cloud-sql' or 'cloud-sql/references/ddl/create.md')"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");

        match self.service.read_skill(&ctx.user_id, path) {
            Some(content) => {
                let raw_size = content.len();

                let sanitized = sanitize_skill_content(&content);
                let sanitized_size = sanitized.content.len();
                if !sanitized.warnings.is_empty() {
                    let labels: Vec<&str> = sanitized.warnings.iter().map(|w| w.pattern).collect();
                    slog!(warn, "skill", "sanitized",
                        path,
                        raw_size,
                        sanitized_size,
                        patterns = ?labels,
                    );
                }

                let output = if sanitized_size > MAX_SKILL_CONTENT_BYTES {
                    let mut end = MAX_SKILL_CONTENT_BYTES;
                    while end > 0 && !sanitized.content.is_char_boundary(end) {
                        end -= 1;
                    }
                    let truncated = &sanitized.content[..end];
                    let dropped = sanitized_size - end;
                    slog!(
                        warn,
                        "skill",
                        "truncated",
                        path,
                        original_size = sanitized_size,
                        truncated_size = end,
                        dropped_bytes = dropped,
                        max = MAX_SKILL_CONTENT_BYTES,
                    );
                    format!("{truncated}\n\n[... truncated at {end}/{sanitized_size} bytes ...]")
                } else {
                    sanitized.content
                };

                slog!(
                    debug,
                    "skill",
                    "completed",
                    path,
                    output_size = output.len(),
                );
                Ok(ToolResult::ok(output))
            }
            None => {
                slog!(warn, "skill", "not_found", path,);
                Ok(ToolResult::ok(format!("Skill not found: {path}")))
            }
        }
    }
}
