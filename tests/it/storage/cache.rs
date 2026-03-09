use bendclaw::storage::cache::Cache;

// ── Basic get/put ──

#[test]
fn cache_put_and_get() {
    let cache: Cache<String> = Cache::new("test", 10);
    cache.put("key1".into(), "value1".into());
    assert_eq!(cache.get("key1"), Some("value1".to_string()));
}

#[test]
fn cache_get_missing_returns_none() {
    let cache: Cache<String> = Cache::new("test", 10);
    assert_eq!(cache.get("missing"), None);
}

// ── Overwrite ──

#[test]
fn cache_put_overwrites() {
    let cache: Cache<String> = Cache::new("test", 10);
    cache.put("key1".into(), "v1".into());
    cache.put("key1".into(), "v2".into());
    assert_eq!(cache.get("key1"), Some("v2".to_string()));
}

// ── Invalidate ──

#[test]
fn cache_invalidate() {
    let cache: Cache<String> = Cache::new("test", 10);
    cache.put("key1".into(), "value1".into());
    cache.invalidate("key1");
    assert_eq!(cache.get("key1"), None);
}

#[test]
fn cache_invalidate_nonexistent_is_noop() {
    let cache: Cache<String> = Cache::new("test", 10);
    cache.invalidate("nonexistent"); // should not panic
}

// ── Clear ──

#[test]
fn cache_clear() {
    let cache: Cache<String> = Cache::new("test", 10);
    cache.put("a".into(), "1".into());
    cache.put("b".into(), "2".into());
    cache.clear();
    assert_eq!(cache.get("a"), None);
    assert_eq!(cache.get("b"), None);
}

// ── LRU eviction ──

#[test]
fn cache_evicts_lru_on_capacity() {
    let cache: Cache<String> = Cache::new("test", 2);
    cache.put("a".into(), "1".into());
    cache.put("b".into(), "2".into());
    cache.put("c".into(), "3".into()); // evicts "a"
    assert_eq!(cache.get("a"), None);
    assert_eq!(cache.get("b"), Some("2".to_string()));
    assert_eq!(cache.get("c"), Some("3".to_string()));
}

#[test]
fn cache_access_refreshes_lru() {
    let cache: Cache<String> = Cache::new("test", 2);
    cache.put("a".into(), "1".into());
    cache.put("b".into(), "2".into());
    // Access "a" to make it recently used
    let _ = cache.get("a");
    cache.put("c".into(), "3".into()); // evicts "b" (least recently used)
    assert_eq!(cache.get("a"), Some("1".to_string()));
    assert_eq!(cache.get("b"), None);
    assert_eq!(cache.get("c"), Some("3".to_string()));
}

// ── Stats ──

#[test]
fn cache_stats_initial() {
    let cache: Cache<String> = Cache::new("my_cache", 10);
    let stats = cache.stats();
    assert_eq!(stats.name, "my_cache");
    assert_eq!(stats.size, 0);
    assert_eq!(stats.capacity, 10);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.hit_rate, 0.0);
}

#[test]
fn cache_stats_tracks_hits_and_misses() {
    let cache: Cache<String> = Cache::new("test", 10);
    cache.put("a".into(), "1".into());

    let _ = cache.get("a"); // hit
    let _ = cache.get("a"); // hit
    let _ = cache.get("b"); // miss

    let stats = cache.stats();
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 1);
    assert!((stats.hit_rate - 2.0 / 3.0).abs() < f64::EPSILON);
    assert_eq!(stats.size, 1);
}

// ── Different value types ──

#[test]
fn cache_with_integer_values() {
    let cache: Cache<i64> = Cache::new("int_cache", 5);
    cache.put("count".into(), 42);
    assert_eq!(cache.get("count"), Some(42));
}

#[test]
fn cache_with_vec_values() {
    let cache: Cache<Vec<u8>> = Cache::new("vec_cache", 5);
    cache.put("data".into(), vec![1, 2, 3]);
    assert_eq!(cache.get("data"), Some(vec![1, 2, 3]));
}

// ── Zero capacity defaults to 256 ──

#[test]
fn cache_zero_capacity_uses_default() {
    let cache: Cache<String> = Cache::new("test", 0);
    let stats = cache.stats();
    assert_eq!(stats.capacity, 256);
}
