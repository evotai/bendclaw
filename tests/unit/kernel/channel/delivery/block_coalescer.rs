use bendclaw::kernel::channels::egress::block_coalescer::BlockCoalescer;

#[test]
fn push_returns_none_below_max() {
    let mut c = BlockCoalescer::new(800, 1200);
    let result = c.push("hello");
    assert!(result.is_none());
    assert!(!c.is_empty());
}

#[test]
fn push_returns_block_when_max_exceeded() {
    let mut c = BlockCoalescer::new(5, 10);
    let chunk = "a".repeat(11);
    let result = c.push(&chunk);
    assert!(result.is_some());
    assert_eq!(result.unwrap().len(), 11);
    assert!(c.is_empty());
}

#[test]
fn flush_if_ready_returns_none_below_min() {
    let mut c = BlockCoalescer::new(100, 200);
    c.push("short");
    let result = c.flush_if_ready();
    assert!(result.is_none());
    assert!(!c.is_empty());
}

#[test]
fn flush_if_ready_returns_block_at_min() {
    let mut c = BlockCoalescer::new(5, 20);
    c.push("hello world");
    let result = c.flush_if_ready();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), "hello world");
    assert!(c.is_empty());
}

#[test]
fn take_drains_buffer() {
    let mut c = BlockCoalescer::new(10, 100);
    c.push("abc");
    let taken = c.take();
    assert_eq!(taken, "abc");
    assert!(c.is_empty());
}

#[test]
fn is_empty_on_new_coalescer() {
    let c = BlockCoalescer::new(100, 200);
    assert!(c.is_empty());
}

#[test]
fn multiple_pushes_accumulate() {
    let mut c = BlockCoalescer::new(100, 200);
    c.push("foo");
    c.push("bar");
    assert!(!c.is_empty());
    let taken = c.take();
    assert_eq!(taken, "foobar");
}
