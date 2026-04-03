use std::sync::Arc;

use crate::storage::runs::RunRepo;
use crate::types::Result;

pub enum CleanupPolicy {
    Full,
    TargetedSession(String),
    Skip,
}

/// Run cleanup before session binding on --resume/--continue.
///
/// Server mode runs Full cleanup at startup.
/// CLI agent mode skips global cleanup; targeted recovery happens here
/// on --resume/--continue.
pub async fn recovery_gate(
    run_repo: &Arc<dyn RunRepo>,
    user_id: &str,
    agent_id: &str,
    policy: CleanupPolicy,
) -> Result<()> {
    match policy {
        CleanupPolicy::Skip => Ok(()),
        CleanupPolicy::Full => {
            let incomplete = run_repo.list_incomplete_runs(user_id, agent_id).await?;
            for run in &incomplete {
                run_repo
                    .clear_handoff(user_id, agent_id, &run.session_id, &run.run_id)
                    .await?;
            }
            Ok(())
        }
        CleanupPolicy::TargetedSession(session_id) => {
            let runs = run_repo
                .list_runs_by_session(user_id, agent_id, &session_id)
                .await?;
            for run in &runs {
                if run.status == "RUNNING" || run.status == "PENDING" {
                    run_repo
                        .clear_handoff(user_id, agent_id, &session_id, &run.run_id)
                        .await?;
                }
            }
            Ok(())
        }
    }
}
