use std::str::FromStr;

use chrono::Utc;
use cron::Schedule;
use serde::Deserialize;
use serde::Serialize;

use crate::observability::log::slog;
use crate::storage::sql;
use crate::types::ErrorCode;
use crate::types::Result as BaseResult;

/// Parse a cron expression, auto-padding 5-field user input to the 7-field
/// format required by the `cron` crate (prepend seconds, append year).
fn parse_cron(expr: &str) -> Result<Schedule, cron::error::Error> {
    Schedule::from_str(expr).or_else(|_| {
        let padded = format!("0 {expr} *");
        Schedule::from_str(&padded)
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskSchedule {
    Cron { expr: String, tz: Option<String> },
    Every { seconds: i32 },
    At { time: String },
}

impl TaskSchedule {
    /// Validate the schedule configuration.
    pub fn validate(&self) -> std::result::Result<(), String> {
        match self {
            TaskSchedule::Cron { expr, tz } => {
                if expr.is_empty() {
                    return Err("schedule.expr is required".into());
                }
                parse_cron(expr).map_err(|e| format!("invalid cron expression: {e}"))?;
                if let Some(tz_name) = tz {
                    tz_name
                        .parse::<chrono_tz::Tz>()
                        .map_err(|_| format!("unknown timezone '{tz_name}'"))?;
                }
                Ok(())
            }
            TaskSchedule::Every { seconds } => {
                if *seconds <= 0 {
                    return Err("schedule.seconds must be > 0".into());
                }
                Ok(())
            }
            TaskSchedule::At { time } => {
                if time.is_empty() {
                    return Err("schedule.time is required".into());
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
            TaskSchedule::Cron { expr, tz } => {
                if expr.is_empty() {
                    return None;
                }
                let schedule = match parse_cron(expr) {
                    Ok(s) => s,
                    Err(e) => {
                        slog!(warn, "task", "invalid_cron", cron_expr = expr, error = %e,);
                        return None;
                    }
                };
                match tz.as_deref().and_then(|s| s.parse::<chrono_tz::Tz>().ok()) {
                    Some(timezone) => schedule.upcoming(timezone).next().map(|dt| {
                        dt.with_timezone(&Utc)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    }),
                    None => schedule
                        .upcoming(Utc)
                        .next()
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string()),
                }
            }
        }
    }

    pub fn from_storage(raw: &str, label: &str) -> BaseResult<Self> {
        if raw.trim().is_empty() || raw.eq_ignore_ascii_case("null") {
            return Err(ErrorCode::storage_serde(format!(
                "{label}: task schedule is empty"
            )));
        }
        sql::parse_json(raw, label)
    }

    pub fn to_storage_expr(&self) -> BaseResult<String> {
        let json = serde_json::to_string(self)
            .map_err(|e| ErrorCode::storage_serde(format!("serialize task schedule: {e}")))?;
        Ok(format!("PARSE_JSON('{}')", sql::escape(&json)))
    }

    /// Compute initial next_run_at when creating a task.
    /// For "at" tasks, returns the specified time. Others delegate to next_run_at.
    pub fn initial_next_run_at(&self) -> Option<String> {
        match self {
            TaskSchedule::At { time } => Some(time.clone()),
            _ => self.next_run_at(),
        }
    }
}
