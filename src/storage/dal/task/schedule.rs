use std::str::FromStr;

use chrono::Utc;
use cron::Schedule;

use super::record::TaskRecord;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskSchedule {
    Cron { expr: String, tz: Option<String> },
    Every { seconds: i32 },
    At { time: String },
}

impl TaskSchedule {
    /// Construct from flat record fields. Returns None if schedule_kind is unrecognized.
    pub fn from_record(
        schedule_kind: &str,
        cron_expr: &str,
        every_seconds: Option<i32>,
        at_time: Option<&str>,
        tz: Option<&str>,
    ) -> Option<Self> {
        match schedule_kind {
            "cron" => Some(TaskSchedule::Cron {
                expr: cron_expr.to_string(),
                tz: tz.map(|s| s.to_string()),
            }),
            "every" => Some(TaskSchedule::Every {
                seconds: every_seconds.unwrap_or(60),
            }),
            "at" => Some(TaskSchedule::At {
                time: at_time.unwrap_or_default().to_string(),
            }),
            _ => None,
        }
    }

    /// Validate the schedule configuration.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            TaskSchedule::Cron { expr, .. } => {
                if expr.is_empty() {
                    return Err("cron expression is required".into());
                }
                Schedule::from_str(expr).map_err(|e| format!("invalid cron expression: {e}"))?;
                Ok(())
            }
            TaskSchedule::Every { seconds } => {
                if *seconds <= 0 {
                    return Err("every_seconds must be > 0".into());
                }
                Ok(())
            }
            TaskSchedule::At { time } => {
                if time.is_empty() {
                    return Err("at_time is required".into());
                }
                Ok(())
            }
        }
    }
    /// Compute next run time after execution. Returns None for one-shot "at" tasks.
    pub fn next_run_at(&self) -> Option<String> {
        match self {
            TaskSchedule::Every { seconds } => {
                let secs = *seconds as i64;
                let next = Utc::now() + chrono::Duration::seconds(secs);
                Some(next.format("%Y-%m-%d %H:%M:%S").to_string())
            }
            TaskSchedule::At { .. } => None,
            TaskSchedule::Cron { expr, .. } => {
                if expr.is_empty() {
                    return None;
                }
                match Schedule::from_str(expr) {
                    Ok(schedule) => schedule
                        .upcoming(Utc)
                        .next()
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string()),
                    Err(e) => {
                        tracing::warn!(cron_expr = expr, error = %e, "invalid cron expression");
                        None
                    }
                }
            }
        }
    }

    /// Compute initial next_run_at when creating a task.
    /// For "at" tasks, returns the specified time. Others delegate to next_run_at.
    pub fn initial_next_run_at(&self) -> Option<String> {
        match self {
            TaskSchedule::At { time } => Some(time.clone()),
            _ => self.next_run_at(),
        }
    }

    /// Write schedule fields back onto a TaskRecord.
    pub fn apply_to_record(&self, record: &mut TaskRecord) {
        record.schedule_kind = self.kind_str().to_string();
        match self {
            TaskSchedule::Cron { expr, tz } => {
                record.cron_expr = expr.clone();
                record.tz = tz.clone();
                record.every_seconds = None;
                record.at_time = None;
            }
            TaskSchedule::Every { seconds } => {
                record.cron_expr = String::new();
                record.tz = None;
                record.every_seconds = Some(*seconds);
                record.at_time = None;
            }
            TaskSchedule::At { time } => {
                record.cron_expr = String::new();
                record.tz = None;
                record.every_seconds = None;
                record.at_time = Some(time.clone());
            }
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match self {
            TaskSchedule::Cron { .. } => "cron",
            TaskSchedule::Every { .. } => "every",
            TaskSchedule::At { .. } => "at",
        }
    }
}
