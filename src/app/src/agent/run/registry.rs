use std::collections::HashMap;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::RunControl;

struct ActiveRun {
    run_id: String,
    handle: RunControl,
    completed: CancellationToken,
}

/// Owns active-run identity and completion races. Lifecycle admission stays
/// with the caller's session gate; this registry never awaits while locked.
#[derive(Default)]
pub struct RunRegistry {
    active: Mutex<HashMap<String, ActiveRun>>,
}

impl RunRegistry {
    pub fn register(
        &self,
        session_id: String,
        run_id: String,
        handle: RunControl,
        completed: CancellationToken,
    ) {
        let mut active = self.active.lock();
        if !completed.is_cancelled() {
            active.insert(session_id, ActiveRun {
                run_id,
                handle,
                completed,
            });
        }
    }

    pub fn complete(&self, session_id: &str, run_id: &str, completed: &CancellationToken) {
        completed.cancel();
        let mut active = self.active.lock();
        if active
            .get(session_id)
            .is_some_and(|run| run.run_id == run_id)
        {
            active.remove(session_id);
        }
    }

    pub fn contains(&self, session_id: &str) -> bool {
        let mut active = self.active.lock();
        if active
            .get(session_id)
            .is_some_and(|run| run.completed.is_cancelled())
        {
            active.remove(session_id);
        }
        active.contains_key(session_id)
    }

    pub fn steer(&self, session_id: &str, message: evot_engine::AgentMessage) {
        self.try_steer(session_id, message);
    }

    /// Atomically check admission and enqueue. A separate contains/steer pair
    /// can report success after completion removed the run.
    pub fn try_steer(&self, session_id: &str, message: evot_engine::AgentMessage) -> bool {
        if let Some(run) = self.active.lock().get(session_id) {
            if !run.completed.is_cancelled() && !run.handle.is_cancelled() {
                run.handle.steer(message);
                return true;
            }
        }
        false
    }

    pub fn follow_up(&self, session_id: &str, message: evot_engine::AgentMessage) {
        if let Some(run) = self.active.lock().get(session_id) {
            if !run.completed.is_cancelled() {
                run.handle.follow_up(message);
            }
        }
    }

    /// Request cancellation and return the completion signal to wait on.
    pub fn abort(&self, session_id: &str) -> Option<CancellationToken> {
        self.active.lock().get(session_id).map(|run| {
            run.handle.abort();
            run.completed.clone()
        })
    }
}
