use bendclaw::storage::time::now;

#[test]
fn now_returns_recent_timestamp() {
    let ts = now();
    let year = ts.format("%Y").to_string().parse::<u32>().unwrap();
    assert!(year >= 2025);
}

#[test]
fn now_is_monotonic() {
    let a = now();
    let b = now();
    assert!(b >= a);
}
