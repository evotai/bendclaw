use std::sync::Arc;

use bendclaw::runtime::ActivityTracker;

#[test]
fn track_task_increments_and_guard_drop_decrements() {
    let tracker = Arc::new(ActivityTracker::new());
    assert!(tracker.is_idle());
    assert_eq!(tracker.active_task_count(), 0);

    let g1 = tracker.track_task();
    assert_eq!(tracker.active_task_count(), 1);
    assert!(!tracker.is_idle());

    let g2 = tracker.track_task();
    assert_eq!(tracker.active_task_count(), 2);

    drop(g1);
    assert_eq!(tracker.active_task_count(), 1);

    drop(g2);
    assert!(tracker.is_idle());
}
