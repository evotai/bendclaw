use std::sync::Arc;

use async_trait::async_trait;

use super::task_label;
use super::ProcessManager;
use crate::types::AgentTool;
use crate::types::Content;
use crate::types::Retention;
use crate::types::ToolContext;
use crate::types::ToolError;
use crate::types::ToolResult;

pub struct TaskStopTool {
    manager: Arc<ProcessManager>,
}

impl TaskStopTool {
    pub fn new(manager: Arc<ProcessManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl AgentTool for TaskStopTool {
    fn name(&self) -> &str {
        "task_stop"
    }

    fn name_aliases(&self) -> Vec<(String, String)> {
        vec![("claude".into(), "TaskStop".into())]
    }

    fn label(&self) -> &str {
        "Stop Task"
    }

    fn description(&self) -> &str {
        "Stop a running background command by task ID."
    }

    fn prompt_snippet(&self) -> Option<&str> {
        Some("Stop a running background command")
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Background task ID to stop" }
            },
            "required": ["task_id"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let task_id = params["task_id"].as_str()?;
        // Name the task being stopped so the card says *what* is being killed,
        // not just that a stop was issued. A short label is enough: the bash
        // card that started the task already printed the command in full.
        match self.manager.summary(task_id) {
            Some(summary) => Some(task_label(&summary.command)),
            None => Some(task_id.to_string()),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let task_id = params["task_id"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'task_id' parameter".into()))?;
        let snapshot = match tokio::select! {
            _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
            snapshot = self.manager.stop(task_id) => snapshot,
        } {
            Some(snapshot) => snapshot,
            // Nothing to stop, but the reason matters: an already-finished task
            // is not the same as a bad id.
            None => {
                return Err(ToolError::Failed(
                    self.manager.missing_task_message(task_id),
                ))
            }
        };
        if snapshot.status.is_terminal() {
            self.manager.claim_notification(task_id);
        }

        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!(
                    "Task {} is {}. Output: {}",
                    snapshot.task_id,
                    snapshot.status.as_str(),
                    snapshot.output_path.display()
                ),
            }],
            details: serde_json::json!({
                "task_id": snapshot.task_id,
                // Same reason as task_output: the card needs to name the task.
                "command": snapshot.command,
                "elapsed_ms": snapshot.elapsed.as_millis(),
                "status": snapshot.status.as_str(),
                "exit_code": snapshot.exit_code,
                "stopped_by_user": snapshot.stopped_by_user,
                "output_path": snapshot.output_path,
            }),
            retention: Retention::Normal,
        })
    }
}
