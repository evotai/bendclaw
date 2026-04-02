use bendclaw::kernel::skills::sources::hub::paths;
use bendclaw::kernel::skills::sources::hub::sync::mark_synced;
use bendclaw::kernel::skills::sources::hub::sync::should_sync;
use tempfile::TempDir;

#[test]
fn should_sync_true_when_no_marker() {
    let tmp = TempDir::new().unwrap();
    let hub = paths::hub_dir(tmp.path());
    std::fs::create_dir_all(&hub).unwrap();
    assert!(should_sync(&hub, 3600));
}

#[test]
fn should_sync_false_when_recently_synced() {
    let tmp = TempDir::new().unwrap();
    let hub = paths::hub_dir(tmp.path());
    std::fs::create_dir_all(&hub).unwrap();
    mark_synced(&hub).unwrap();
    assert!(!should_sync(&hub, 3600));
}
