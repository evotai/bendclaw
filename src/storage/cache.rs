use std::num::NonZeroUsize;
use std::time::Duration;
use std::time::Instant;

use lru::LruCache;
use parking_lot::Mutex;

/// LRU cache with per-entry TTL expiration.
pub struct TtlCache<V: Clone> {
    inner: Mutex<LruCache<String, TtlEntry<V>>>,
    ttl: Duration,
    name: String,
    hits: Mutex<u64>,
    misses: Mutex<u64>,
}

#[derive(Clone)]
struct TtlEntry<V: Clone> {
    value: V,
    inserted_at: Instant,
}

impl<V: Clone> TtlCache<V> {
    pub fn new(name: &str, capacity: usize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(LruCache::new(
                NonZeroUsize::new(capacity)
                    .unwrap_or_else(|| NonZeroUsize::new(256).expect("256 is non-zero")),
            )),
            ttl,
            name: name.to_string(),
            hits: Mutex::new(0),
            misses: Mutex::new(0),
        }
    }

    pub fn get(&self, key: &str) -> Option<V> {
        let mut cache = self.inner.lock();
        if let Some(entry) = cache.get(key) {
            if entry.inserted_at.elapsed() < self.ttl {
                *self.hits.lock() += 1;
                return Some(entry.value.clone());
            }
            cache.pop(key);
        }
        *self.misses.lock() += 1;
        None
    }

    pub fn put(&self, key: String, value: V) {
        self.inner.lock().put(key, TtlEntry {
            value,
            inserted_at: Instant::now(),
        });
    }

    pub fn clear(&self) {
        self.inner.lock().clear();
    }

    pub fn stats(&self) -> CacheStats {
        let cache = self.inner.lock();
        let hits = *self.hits.lock();
        let misses = *self.misses.lock();
        CacheStats {
            name: self.name.clone(),
            size: cache.len(),
            capacity: cache.cap().get(),
            hits,
            misses,
            hit_rate: if hits + misses > 0 {
                hits as f64 / (hits + misses) as f64
            } else {
                0.0
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheStats {
    pub name: String,
    pub size: usize,
    pub capacity: usize,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f64,
}
