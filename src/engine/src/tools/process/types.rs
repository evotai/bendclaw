use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use tokio::process::Command;

/// Interval between user-facing progress updates while a task is watched.
///
/// Shared by `bash`'s foreground wait and `task_output`'s blocking wait so a
/// watched task ticks at the same rate however it is being watched.
pub const PROGRESS_INTERVAL: Duration = Duration::from_secs(3);
/// Interval between partial output updates while a task is watched.
pub const UPDATE_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundReason {
    Explicit,
    YieldElapsed,
    UserRequested,
    MessageDelivery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
pub enum ProcessStatus {
    RunningForeground,
    RunningBackground(BackgroundReason),
    Completed,
    Failed,
    TimedOut,
    Killed,
}

impl ProcessStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::TimedOut | Self::Killed
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RunningForeground => "running_foreground",
            Self::RunningBackground(_) => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Killed => "killed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub task_id: String,
    pub tool_call_id: String,
    pub command: String,
    pub cwd: PathBuf,
    pub output_path: PathBuf,
    pub output: String,
    pub output_file_bytes: usize,
    pub output_file_truncated: bool,
    pub total_lines: usize,
    pub status: ProcessStatus,
    pub exit_code: Option<i32>,
    pub elapsed: Duration,
    /// True when the stop came from the user (`/stop`, background panel) rather
    /// than from the model's own `task_stop` call.
    ///
    /// `Killed` alone does not say who killed it, which left the model guessing
    /// — and guessing wrong, e.g. blaming turn teardown for a task the user had
    /// stopped by hand.
    pub stopped_by_user: bool,
}

/// Listing-only view of a task. Deliberately excludes the captured output so
/// polling callers (the TUI footer) never copy megabytes of rolling tail just
/// to count what is running.
#[derive(Debug, Clone)]
pub struct ProcessSummary {
    pub task_id: String,
    pub command: String,
    pub cwd: PathBuf,
    pub output_path: PathBuf,
    pub status: ProcessStatus,
    pub exit_code: Option<i32>,
    pub elapsed: Duration,
    pub output_file_truncated: bool,
    /// See `ProcessSnapshot::stopped_by_user`.
    pub stopped_by_user: bool,
}

pub struct StartProcess {
    pub command: Command,
    pub command_text: String,
    pub tool_call_id: String,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub output_dir: PathBuf,
    pub tail_bytes: usize,
    pub background_reason: Option<BackgroundReason>,
}
