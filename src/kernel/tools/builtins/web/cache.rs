use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use dashmap::DashMap;

struct CacheEntry {
    value: String,
    inserted_at: Instant,
}

/// Thread-safe search result cache with TTL expiry.
///
/// Set `ttl` to `Duration::ZERO` to disable caching (all lookups miss).
#[derive(Clone)]
pub struct WebCache {
    ttl: Duration,
    entries: Arc<DashMap<String, CacheEntry>>,
}

impl WebCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Arc::new(DashMap::new()),
        }
    }

    /// Build a cache key for search results.
    pub fn search_key(query: &str, count: u32) -> String {
        format!("search:{query}:{count}")
    }

    /// Return cached value if present and not expired.
    pub fn get(&self, key: &str) -> Option<String> {
        if self.ttl.is_zero() {
            return None;
        }
        let entry = self.entries.get(key)?;
        if entry.inserted_at.elapsed() < self.ttl {
            Some(entry.value.clone())
        } else {
            drop(entry);
            self.entries.remove(key);
            None
        }
    }

    /// Insert a value into the cache.
    pub fn insert(&self, key: String, value: String) {
        if self.ttl.is_zero() {
            return;
        }
        self.entries.insert(key, CacheEntry {
            value,
            inserted_at: Instant::now(),
        });
    }
}
