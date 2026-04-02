use serde::Serialize;

/// Structured lifecycle events at run boundaries — consumed by monitoring.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event")]
pub enum LifecycleEvent {
    #[serde(rename = "run.started")]
    RunStarted {
        run_id: String,
        session_id: String,
        agent_id: String,
        user_id: String,
    },
    #[serde(rename = "run.resumed")]
    RunResumed {
        run_id: String,
        session_id: String,
        parent_run_id: String,
    },
    #[serde(rename = "run.completed")]
    RunCompleted {
        run_id: String,
        session_id: String,
        iterations: u32,
        stop_reason: String,
    },
    #[serde(rename = "run.failed")]
    RunFailed {
        run_id: String,
        session_id: String,
        error: String,
    },
    #[serde(rename = "run.interrupted")]
    RunInterrupted {
        run_id: String,
        session_id: String,
        reason: String,
    },
    #[serde(rename = "cleanup.started")]
    CleanupStarted {
        user_id: String,
        agent_id: String,
        policy: String,
    },
    #[serde(rename = "cleanup.completed")]
    CleanupCompleted {
        user_id: String,
        agent_id: String,
        cleaned: usize,
    },
}

impl LifecycleEvent {
    pub fn emit(&self) {
        if let Ok(json) = serde_json::to_string(self) {
            tracing::info!(target: "bendclaw::lifecycle", "{}", json);
        }
    }
}
