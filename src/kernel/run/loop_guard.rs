//! Detects repetitive tool call patterns and prevents infinite loops.

use std::collections::VecDeque;

/// Configuration for the loop guard.
#[derive(Debug, Clone)]
pub struct LoopGuardConfig {
    /// Max identical (name, args) calls allowed in the sliding window.
    pub max_identical_calls: u32,
    /// Size of the sliding window.
    pub window_size: usize,
    /// Total blocked calls before global circuit breaker trips.
    pub circuit_breaker: u32,
}

impl Default for LoopGuardConfig {
    fn default() -> Self {
        Self {
            max_identical_calls: 3,
            window_size: 10,
            circuit_breaker: 8,
        }
    }
}

/// Verdict from the loop guard for a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopGuardVerdict {
    Allow,
    Warn(String),
    Block(String),
}

/// Tracks tool call history and detects repetitive patterns.
#[derive(Debug)]
pub struct LoopGuard {
    config: LoopGuardConfig,
    history: VecDeque<(String, String)>,
    total_blocked: u32,
}

impl LoopGuard {
    pub fn new(config: LoopGuardConfig) -> Self {
        Self {
            history: VecDeque::with_capacity(config.window_size + 1),
            config,
            total_blocked: 0,
        }
    }

    /// Check whether a tool call should be allowed, warned, or blocked.
    pub fn check(&mut self, name: &str, args: &serde_json::Value) -> LoopGuardVerdict {
        if self.total_blocked >= self.config.circuit_breaker {
            self.total_blocked += 1;
            return LoopGuardVerdict::Block(format!(
                "circuit breaker tripped: {} total blocked calls",
                self.total_blocked,
            ));
        }

        let canonical = canonicalize(name, args);
        let count = self
            .history
            .iter()
            .filter(|(n, a)| *n == canonical.0 && *a == canonical.1)
            .count() as u32;

        if count >= self.config.max_identical_calls {
            self.total_blocked += 1;
            return LoopGuardVerdict::Block(format!(
                "tool '{}' called {} times with identical arguments in last {} calls",
                name,
                count + 1,
                self.config.window_size,
            ));
        }

        if count + 1 == self.config.max_identical_calls {
            return LoopGuardVerdict::Warn(format!(
                "tool '{}' called {} times with identical arguments, next identical call will be blocked",
                name,
                count + 1,
            ));
        }

        LoopGuardVerdict::Allow
    }

    /// Record a tool call into the sliding window.
    pub fn record(&mut self, name: &str, args: &serde_json::Value) {
        let canonical = canonicalize(name, args);
        self.history.push_back(canonical);
        while self.history.len() > self.config.window_size {
            self.history.pop_front();
        }
    }
}

impl Default for LoopGuard {
    fn default() -> Self {
        Self::new(LoopGuardConfig::default())
    }
}

/// Produce a canonical (name, sorted-keys JSON) pair for comparison.
fn canonicalize(name: &str, args: &serde_json::Value) -> (String, String) {
    (name.to_string(), canonical_json(args))
}

fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let entries: Vec<String> = keys
                .iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::json!(k),
                        canonical_json(&map[k.as_str()])
                    )
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(canonical_json).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}
