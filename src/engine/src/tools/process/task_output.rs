use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;

use super::task_label;
use super::ProcessManager;
use super::ProcessSnapshot;
use super::PROGRESS_INTERVAL;
use super::UPDATE_INTERVAL;
use crate::types::AgentTool;
use crate::types::Content;
use crate::types::Retention;
use crate::types::ToolContext;
use crate::types::ToolError;
use crate::types::ToolResult;

/// Bound on a blocking wait, when a host asks for one.
///
/// Unset by default, for the same reason `bash` no longer slices its foreground
/// wait: `ctrl+b` and typing a message both release a blocking wait, so the turn
/// is one keypress away and a timer adds nothing. What the timer did add was a
/// loop — the wait returned `timeout` with the task still running, which reads
/// as "ask again", so one wait became a series of them.
///
/// This interaction policy belongs to the runtime. It is intentionally not an
/// AI-visible parameter, so a tool call cannot extend or disable the bound.
const NO_BLOCKING_WAIT_LIMIT: Option<Duration> = None;

pub struct TaskOutputTool {
    manager: Arc<ProcessManager>,
    blocking_wait_limit: Option<Duration>,
}

impl TaskOutputTool {
    pub fn new(manager: Arc<ProcessManager>) -> Self {
        Self {
            manager,
            blocking_wait_limit: NO_BLOCKING_WAIT_LIMIT,
        }
    }

    /// Bound the blocking wait, which is unbounded by default.
    ///
    /// This is configured by the host rather than exposed in the tool schema.
    /// Tests use it to reach the bounded path without waiting one out.
    pub fn with_blocking_wait_limit(mut self, limit: Duration) -> Self {
        self.blocking_wait_limit = Some(limit);
        self
    }

    /// Wait for a task to finish, reporting progress while it runs.
    ///
    /// `ProcessManager::wait` is silent, which left a blocking poll rendering a
    /// single frozen `waiting for task` line: no ticking clock and no new
    /// output, so a live task was indistinguishable from a hung one and users
    /// interrupted work that was fine. Mirrors `bash`'s foreground loop, and
    /// shares its intervals so a watched task ticks at one rate.
    ///
    /// `Ok(None)` means the task id is unknown; the caller turns that into the
    /// not-found error.
    async fn watch(
        &self,
        task_id: &str,
        timeout: Option<Duration>,
        ctx: &ToolContext,
    ) -> Result<Option<(ProcessSnapshot, bool)>, ToolError> {
        let started = Instant::now();
        let mut last_progress = Instant::now();
        let mut last_update = Instant::now();
        let mut reported_lines = 0usize;
        // A blocking wait holds the whole turn while the task it watches is
        // already backgrounded, so there is no foreground shell for the user to
        // detach. Registering the wait lets the UI see that state, and the
        // generation lets it end the wait without cancelling the run.
        let _wait_guard = self.manager.enter_blocking_wait();
        let release_generation = self.manager.wait_release_generation();
        loop {
            if ctx.cancel.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            let Some(snapshot) = self.manager.snapshot(task_id) else {
                return Ok(None);
            };
            let elapsed = started.elapsed();
            if snapshot.status.is_terminal() || timeout.is_some_and(|limit| elapsed >= limit) {
                return Ok(Some((snapshot, false)));
            }
            // The user reclaimed the turn. Hand back what the task looks like now
            // rather than erroring: the command keeps running, so this reads as a
            // wait that ended early, not as a failure.
            if self.manager.wait_release_generation() != release_generation {
                return Ok(Some((snapshot, true)));
            }

            if elapsed >= PROGRESS_INTERVAL && last_progress.elapsed() >= PROGRESS_INTERVAL {
                if let Some(on_progress) = &ctx.on_progress {
                    on_progress(format!("Waiting... {}s", elapsed.as_secs()));
                }
                last_progress = Instant::now();
            }
            // Only forward output the caller has not seen yet, so a long tail is
            // not re-sent every couple of seconds.
            if elapsed >= UPDATE_INTERVAL
                && last_update.elapsed() >= UPDATE_INTERVAL
                && snapshot.total_lines > reported_lines
            {
                if let Some(on_update) = &ctx.on_update {
                    if !snapshot.output.is_empty() {
                        on_update(ToolResult {
                            content: vec![Content::Text {
                                text: snapshot.output,
                            }],
                            details: serde_json::Value::Null,
                            retention: Retention::Normal,
                        });
                    }
                }
                reported_lines = snapshot.total_lines;
                last_update = Instant::now();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[async_trait]
impl AgentTool for TaskOutputTool {
    fn name(&self) -> &str {
        "task_output"
    }

    fn name_aliases(&self) -> Vec<(String, String)> {
        vec![("claude".into(), "TaskOutput".into())]
    }

    fn label(&self) -> &str {
        "Task Output"
    }

    fn description(&self) -> &str {
        // States what each path costs and stops there. An earlier version ranked
        // them ("reading is usually better"), which is not this tool's call to
        // make: waiting on a task whose result the next step needs is what this
        // tool is for, and framing it as the inferior option told a model its
        // legitimate use was a mistake.
        "Get status and recent output from a background command. Waits for the task to finish by default, returning when it ends or when the user reclaims the turn. Pass block: false for an immediate snapshot. The task's output file is also readable directly at the path the command returned."
    }

    fn prompt_snippet(&self) -> Option<&str> {
        Some("Read status or wait for a background command")
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "Background task ID" },
                // Prices blocking without disparaging it. The cost is real and a
                // model should know it holds the turn; calling it "throwing away
                // the point" of backgrounding went further and framed the tool's
                // primary use as a misuse.
                "block": { "type": "boolean", "description": "Wait for the task to finish (default true). The wait ends when the task finishes or the user reclaims the turn; pass false for an immediate status snapshot." }
            },
            "required": ["task_id"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let task_id = params["task_id"].as_str()?;
        // Name the task being polled, so several concurrent task_output cards
        // are distinguishable the moment they start — the result details are
        // not available yet while the call is running. A short label, not the
        // whole command: the bash card that started the task already printed it
        // in full, and repeating a long pipeline on every poll only wraps.
        match self.manager.summary(task_id) {
            Some(summary) => Some(task_label(&summary.command)),
            // Unknown id (already forgotten, or a model typo): show the id
            // rather than nothing, so the card still says what was asked for.
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
        let block = params["block"].as_bool().is_none_or(|value| value);

        let (snapshot, released) = match if block {
            self.watch(task_id, self.blocking_wait_limit, &ctx).await?
        } else {
            self.manager
                .snapshot(task_id)
                .map(|snapshot| (snapshot, false))
        } {
            Some(pair) => pair,
            // A reaped task ran to completion; saying "not found" invites
            // re-running work that already succeeded.
            None => {
                return Err(ToolError::Failed(
                    self.manager.missing_task_message(task_id),
                ))
            }
        };

        let retrieval_status = if snapshot.status.is_terminal() {
            self.manager.claim_notification(task_id);
            "success"
        } else if released {
            // Distinct from `timeout`: nothing went wrong and no deadline was
            // hit, so the model must not read this as the task being slow.
            "released"
        } else if block {
            "timeout"
        } else {
            "not_ready"
        };
        let mut text = format!(
            "Task ID: {}\nStatus: {}\nOutput file: {}",
            snapshot.task_id,
            snapshot.status.as_str(),
            snapshot.output_path.display()
        );
        if let Some(exit_code) = snapshot.exit_code {
            text.push_str(&format!("\nExit code: {exit_code}"));
        }
        if released {
            // Without this the model sees a non-terminal status and reasonably
            // calls task_output again, walking straight back into the wait the
            // user just ended.
            //
            // "Stop polling" is warranted here, unlike in the tool's description:
            // this is not an opinion about which collection path is better, it
            // enforces a decision the user just made with a keypress. Re-waiting
            // would undo it.
            text.push_str(
                "\nThe user ended this wait to get the turn back; the task was not interrupted and is still running. Do not wait on it again unless they ask — stop polling and respond to them now.",
            );
        }
        if snapshot.stopped_by_user {
            // A bare `killed` left the model inferring a cause from process
            // tables and file timestamps, and inferring wrong.
            text.push_str(
                "\nCancelled by the user and won't be resumed. Treat its work as cancelled; only re-run it if the user explicitly asks.",
            );
        }
        if snapshot.output_file_truncated {
            text.push_str(
                "\nOutput file truncated at 10485760 bytes; recent output below is still current.",
            );
        }
        if !snapshot.output.is_empty() {
            text.push_str("\nOutput:\n");
            text.push_str(&snapshot.output);
        }

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::json!({
                "retrieval_status": retrieval_status,
                "task_id": snapshot.task_id,
                // The command and elapsed time let a caller name the task it
                // polled. Without them every task_output card renders
                // identically, which is useless when several are in flight.
                "command": snapshot.command,
                "elapsed_ms": snapshot.elapsed.as_millis(),
                "total_lines": snapshot.total_lines,
                "status": snapshot.status.as_str(),
                "exit_code": snapshot.exit_code,
                "stopped_by_user": snapshot.stopped_by_user,
                "output_path": snapshot.output_path,
                "output_file_bytes": snapshot.output_file_bytes,
                "output_file_truncated": snapshot.output_file_truncated,
            }),
            retention: Retention::Normal,
        })
    }
}
