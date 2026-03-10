use bendclaw::storage::time::now;

#[test]
fn now_returns_recent_timestamp() -> anyhow::Result<()> {
    let ts = now();
    let year = ts.format("%Y").to_string().parse::<u32>()?;
    assert!(year >= 2025);
    Ok(())
}

#[test]
fn now_is_monotonic() {
    let a = now();
    let b = now();
    assert!(b >= a);
}
