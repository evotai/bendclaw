use std::time::Duration;

use bendclaw::kernel::channels::model::status::ChannelStatus;

#[test]
fn reset_and_get() {
    let status = ChannelStatus::new();
    assert!(status.get("acct-1").is_none());

    status.reset(
        "acct-1",
        serde_json::json!({"token": "abc"}),
        Duration::from_secs(60),
    );
    let s = status.get("acct-1").unwrap();
    assert!(s.connected);
    assert!(!s.is_stale());
    assert_eq!(s.config, serde_json::json!({"token": "abc"}));
}

#[test]
fn clear_removes_entry() {
    let status = ChannelStatus::new();
    status.reset("acct-1", serde_json::json!({}), Duration::from_secs(60));
    assert!(status.get("acct-1").is_some());

    status.clear("acct-1");
    assert!(status.get("acct-1").is_none());
}

#[test]
fn set_connected_false() {
    let status = ChannelStatus::new();
    status.reset("acct-1", serde_json::json!({}), Duration::from_secs(60));

    status.set_connected("acct-1", false);
    let s = status.get("acct-1").unwrap();
    assert!(!s.connected);
}

#[test]
fn set_connected_true_updates_last_event() {
    let status = ChannelStatus::new();
    status.reset("acct-1", serde_json::json!({}), Duration::from_secs(60));

    std::thread::sleep(Duration::from_millis(10));
    let before = status.get("acct-1").unwrap().last_event_at;

    status.set_connected("acct-1", true);
    let after = status.get("acct-1").unwrap().last_event_at;
    assert!(after > before);
}

#[test]
fn touch_event_updates_last_event() {
    let status = ChannelStatus::new();
    status.reset("acct-1", serde_json::json!({}), Duration::from_secs(60));

    std::thread::sleep(Duration::from_millis(10));
    let before = status.get("acct-1").unwrap().last_event_at;

    status.touch_event("acct-1");
    let after = status.get("acct-1").unwrap().last_event_at;
    assert!(after > before);
}

#[test]
fn touch_event_noop_for_unknown_account() {
    let status = ChannelStatus::new();
    status.touch_event("nonexistent"); // should not panic
}

#[test]
fn is_stale_with_zero_threshold() {
    let status = ChannelStatus::new();
    status.reset("acct-1", serde_json::json!({}), Duration::ZERO);

    std::thread::sleep(Duration::from_millis(1));
    let s = status.get("acct-1").unwrap();
    assert!(s.is_stale());
}

#[test]
fn not_stale_within_threshold() {
    let status = ChannelStatus::new();
    status.reset("acct-1", serde_json::json!({}), Duration::from_secs(600));

    let s = status.get("acct-1").unwrap();
    assert!(!s.is_stale());
}
