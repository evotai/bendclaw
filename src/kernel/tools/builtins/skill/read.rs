use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::skills::sanitizer::sanitize_skill_content;
use crate::kernel::skills::store::SkillStore;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;

/// Maximum skill content size returned to the LLM (64 KiB).
/// Matches ironclaw's `MAX_PROMPT_FILE_SIZE`.
const MAX_SKILL_CONTENT_BYTES: usize = 64 * 1024;

/// In-process tool that reads skill documentation.
pub struct SkillReadTool {
    store: Arc<SkillStore>,
}

impl SkillReadTool {
    pub fn new(store: Arc<SkillStore>) -> Self {
        Self { store }
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

    fn hint(&self) -> &str {
        "read a skill's full instructions"
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
        tracing::info!(
            stage = "skill_read",
            status = "started",
            path,
            "skill_read started"
        );

        match self.store.read_skill(&ctx.agent_id, path) {
            Some(content) => {
                let raw_size = content.len();
                tracing::info!(
                    stage = "skill_read",
                    status = "loaded",
                    path,
                    raw_size,
                    "skill_read loaded"
                );

                let sanitized = sanitize_skill_content(&content);
                let sanitized_size = sanitized.content.len();
                if !sanitized.warnings.is_empty() {
                    let labels: Vec<&str> = sanitized.warnings.iter().map(|w| w.pattern).collect();
                    tracing::warn!(
                        stage = "skill_read",
                        status = "sanitized",
                        path,
                        raw_size,
                        sanitized_size,
                        patterns = ?labels,
                        "skill_read sanitized"
                    );
                } else {
                    tracing::info!(
                        stage = "skill_read",
                        status = "clean",
                        path,
                        sanitized_size,
                        "skill_read clean"
                    );
                }

                let output = if sanitized_size > MAX_SKILL_CONTENT_BYTES {
                    let mut end = MAX_SKILL_CONTENT_BYTES;
                    while end > 0 && !sanitized.content.is_char_boundary(end) {
                        end -= 1;
                    }
                    let truncated = &sanitized.content[..end];
                    let dropped = sanitized_size - end;
                    tracing::warn!(
                        stage = "skill_read",
                        status = "truncated",
                        path,
                        original_size = sanitized_size,
                        truncated_size = end,
                        dropped_bytes = dropped,
                        max = MAX_SKILL_CONTENT_BYTES,
                        "skill_read truncated"
                    );
                    format!("{truncated}\n\n[... truncated at {end}/{sanitized_size} bytes ...]")
                } else {
                    sanitized.content
                };

                tracing::info!(
                    stage = "skill_read",
                    status = "completed",
                    path,
                    output_size = output.len(),
                    "skill_read completed"
                );
                Ok(ToolResult::ok(output))
            }
            None => {
                tracing::warn!(
                    stage = "skill_read",
                    status = "not_found",
                    path,
                    "skill_read not_found"
                );
                Ok(ToolResult::ok(format!("Skill not found: {path}")))
            }
        }
    }
}
