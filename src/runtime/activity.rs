use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

/// Control-plane summary of whether the runtime can be suspended safely.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SuspendStatus {
    pub can_suspend: bool,
    pub active_sessions: usize,
    pub active_tasks: usize,
    pub active_leases: usize,
}

/// Tracks active runtime-managed background tasks.
#[derive(Debug, Default)]
pub struct ActivityTracker {
    active_tasks: AtomicUsize,
}

impl ActivityTracker {
    pub fn new() -> Self {
        Self {
            active_tasks: AtomicUsize::new(0),
        }
    }

    pub fn track_task(self: &Arc<Self>) -> ActivityGuard {
        self.active_tasks.fetch_add(1, Ordering::Relaxed);
        ActivityGuard {
            tracker: self.clone(),
        }
    }

    pub fn active_task_count(&self) -> usize {
        self.active_tasks.load(Ordering::Relaxed)
    }

    pub fn is_idle(&self) -> bool {
        self.active_task_count() == 0
    }
}

pub struct ActivityGuard {
    tracker: Arc<ActivityTracker>,
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.tracker.active_tasks.fetch_sub(1, Ordering::Relaxed);
    }
}
