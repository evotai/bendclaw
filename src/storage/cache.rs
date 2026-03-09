use std::num::NonZeroUsize;

use lru::LruCache;
use parking_lot::Mutex;

/// Generic write-through LRU cache.
pub struct Cache<V: Clone> {
    inner: Mutex<LruCache<String, V>>,
    name: String,
    hits: Mutex<u64>,
    misses: Mutex<u64>,
}

impl<V: Clone> Cache<V> {
    pub fn new(name: &str, capacity: usize) -> Self {
        Self {
            inner: Mutex::new(LruCache::new(
                NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(256).unwrap()),
            )),
            name: name.to_string(),
            hits: Mutex::new(0),
            misses: Mutex::new(0),
        }
    }

    pub fn get(&self, key: &str) -> Option<V> {
        let mut cache = self.inner.lock();
        if let Some(v) = cache.get(key) {
            *self.hits.lock() += 1;
            Some(v.clone())
        } else {
            *self.misses.lock() += 1;
            None
        }
    }

    pub fn put(&self, key: String, value: V) {
        self.inner.lock().put(key, value);
    }

    pub fn invalidate(&self, key: &str) {
        self.inner.lock().pop(key);
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
