use std::sync::Arc;

use crate::storage::backend::run_repo::RunRepo;
use crate::types::Result;

/// Cleanup policy — determines scope of incomplete run detection.
#[derive(Debug)]
pub enum CleanupPolicy {
    /// Full scan for all incomplete runs under this agent.
    Full,
    /// Only check runs for a specific session.
    TargetedSession(String),
    /// Skip cleanup entirely.
    Skip,
}

/// Detect and clean up incomplete runs.
///
/// Server mode: `Full` at startup before accepting requests.
/// CLI agent mode: `Skip` at startup; `TargetedSession` on --resume/--continue.
pub async fn cleanup(
    run_repo: &Arc<dyn RunRepo>,
    user_id: &str,
    agent_id: &str,
    policy: CleanupPolicy,
) -> Result<CleanupResult> {
    match policy {
        CleanupPolicy::Skip => Ok(CleanupResult { cleaned: 0 }),
        CleanupPolicy::Full => {
            let incomplete = run_repo.list_incomplete_runs(user_id, agent_id).await?;
            let count = incomplete.len();
            for run in &incomplete {
                run_repo
                    .clear_handoff(user_id, agent_id, &run.session_id, &run.run_id)
                    .await?;
            }
            Ok(CleanupResult { cleaned: count })
        }
        CleanupPolicy::TargetedSession(session_id) => {
            let runs = run_repo
                .list_runs_by_session(user_id, agent_id, &session_id)
                .await?;
            let mut count = 0;
            for run in &runs {
                if run.status == "RUNNING" || run.status == "PENDING" {
                    run_repo
                        .clear_handoff(user_id, agent_id, &session_id, &run.run_id)
                        .await?;
                    count += 1;
                }
            }
            Ok(CleanupResult { cleaned: count })
        }
    }
}

#[derive(Debug)]
pub struct CleanupResult {
    pub cleaned: usize,
}
