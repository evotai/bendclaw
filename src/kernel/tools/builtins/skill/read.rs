use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::Result;
use crate::kernel::skills::catalog::SkillCatalog;
use crate::kernel::skills::sanitizer::sanitize_skill_content;
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
    catalog: Arc<dyn SkillCatalog>,
}

impl SkillReadTool {
    pub fn new(catalog: Arc<dyn SkillCatalog>) -> Self {
        Self { catalog }
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
        _ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        tracing::info!(path, "skill_read: request received");

        match self.catalog.read_skill(path) {
            Some(content) => {
                let raw_size = content.len();
                tracing::info!(
                    path,
                    raw_size,
                    "skill_read: raw content loaded from catalog"
                );

                let sanitized = sanitize_skill_content(&content);
                let sanitized_size = sanitized.content.len();
                if !sanitized.warnings.is_empty() {
                    let labels: Vec<&str> = sanitized.warnings.iter().map(|w| w.pattern).collect();
                    tracing::warn!(
                        path,
                        raw_size,
                        sanitized_size,
                        patterns = ?labels,
                        "skill_read: content sanitized — patterns removed"
                    );
                } else {
                    tracing::info!(
                        path,
                        sanitized_size,
                        "skill_read: content clean, no sanitization needed"
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
                        path,
                        original_size = sanitized_size,
                        truncated_size = end,
                        dropped_bytes = dropped,
                        max = MAX_SKILL_CONTENT_BYTES,
                        "skill_read: content TRUNCATED"
                    );
                    format!("{truncated}\n\n[... truncated at {end}/{sanitized_size} bytes ...]")
                } else {
                    sanitized.content
                };

                tracing::info!(
                    path,
                    output_size = output.len(),
                    "skill_read: returning content to LLM"
                );
                Ok(ToolResult::ok(output))
            }
            None => {
                tracing::warn!(path, "skill_read: skill not found in catalog");
                Ok(ToolResult::ok(format!("Skill not found: {path}")))
            }
        }
    }
}
