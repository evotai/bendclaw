use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::base::new_id;
use crate::base::Result;
use crate::kernel::recall::RecallStore;
use crate::kernel::tools::OperationClassifier;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolId;
use crate::kernel::tools::ToolResult;
use crate::kernel::OpType;
use crate::storage::dal::learning::LearningRecord;

pub const VALID_KINDS: &[&str] = &[
    "correction",
    "workflow",
    "retrieval",
    "constraint",
    "pattern",
];

const MAX_CONTENT_BYTES: usize = 2000;

/// Write a reusable agent-level learning.
pub struct LearningWriteTool {
    store: Arc<RecallStore>,
}

impl LearningWriteTool {
    pub fn new(store: Arc<RecallStore>) -> Self {
        Self { store }
    }
}

impl OperationClassifier for LearningWriteTool {
    fn op_type(&self) -> OpType {
        OpType::LearningWrite
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        args.get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[async_trait]
impl Tool for LearningWriteTool {
    fn name(&self) -> &str {
        ToolId::LearningWrite.as_str()
    }

    fn description(&self) -> &str {
        "Write a reusable agent-level learning. Use this for durable corrections, workflows, retrieval tactics, constraints, or patterns. \
         Use memory_write instead for user or session preferences."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": VALID_KINDS,
                    "description": "Learning category"
                },
                "subject": {
                    "type": "string",
                    "description": "What this learning applies to, such as 'databend', 'repo', or 'shell'"
                },
                "title": {
                    "type": "string",
                    "description": "Short learning title"
                },
                "content": {
                    "type": "string",
                    "description": "Reusable lesson in plain English. Summarize the lesson instead of copying raw external content."
                },
                "conditions": {
                    "type": "object",
                    "description": "Optional conditions that explain when the learning applies"
                },
                "strategy": {
                    "type": "object",
                    "description": "Optional structured strategy for applying the learning"
                },
                "priority": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 10,
                    "default": 5,
                    "description": "Relative importance"
                },
                "confidence": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 1,
                    "default": 0.8,
                    "description": "Confidence that this learning is correct and reusable"
                }
            },
            "required": ["kind", "subject", "title", "content"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let kind = args
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let subject = args
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if kind.is_empty() {
            return Ok(ToolResult::error("kind is required"));
        }
        if !VALID_KINDS.contains(&kind) {
            return Ok(ToolResult::error(format!(
                "kind must be one of: {}",
                VALID_KINDS.join(", ")
            )));
        }
        if subject.is_empty() {
            return Ok(ToolResult::error("subject is required"));
        }
        if title.is_empty() {
            return Ok(ToolResult::error("title is required"));
        }
        if content.is_empty() {
            return Ok(ToolResult::error("content is required"));
        }
        if content.len() > MAX_CONTENT_BYTES {
            return Ok(ToolResult::error(format!(
                "content too long: {} bytes (max {})",
                content.len(),
                MAX_CONTENT_BYTES
            )));
        }

        let conditions = match parse_optional_object(&args, "conditions") {
            Ok(value) => value,
            Err(msg) => return Ok(ToolResult::error(msg)),
        };
        let strategy = match parse_optional_object(&args, "strategy") {
            Ok(value) => value,
            Err(msg) => return Ok(ToolResult::error(msg)),
        };

        let priority = args.get("priority").and_then(|v| v.as_i64()).unwrap_or(5);
        if !(0..=10).contains(&priority) {
            return Ok(ToolResult::error("priority must be between 0 and 10"));
        }

        let confidence = args
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.8);
        if !(0.0..=1.0).contains(&confidence) {
            return Ok(ToolResult::error("confidence must be between 0 and 1"));
        }

        let record = LearningRecord {
            id: new_id(),
            kind: kind.to_string(),
            subject: subject.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            conditions,
            strategy,
            priority: priority as i32,
            confidence,
            status: "active".to_string(),
            supersedes_id: String::new(),
            user_id: ctx.user_id.to_string(),
            scope: "shared".to_string(),
            created_by: ctx.user_id.to_string(),
            source_run_id: String::new(),
            success_count: 0,
            failure_count: 0,
            last_applied_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        let op = crate::kernel::writer::tool_op::ToolWriteOp::LearningWrite {
            store: self.store.clone(),
            record: Box::new(record),
        };
        ctx.tool_writer.send(op);

        Ok(ToolResult::ok(format!(
            "Learning '{}' written ({})",
            title, kind
        )))
    }
}

fn parse_optional_object(
    args: &serde_json::Value,
    field: &str,
) -> std::result::Result<Option<serde_json::Value>, String> {
    match args.get(field) {
        None => Ok(None),
        Some(value) if value.is_object() => Ok(Some(value.clone())),
        Some(_) => Err(format!("{field} must be an object")),
    }
}
