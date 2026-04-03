use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;

const DEFAULT_STALE_EVENT_THRESHOLD: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub struct AccountStatus {
    pub connected: bool,
    pub last_event_at: Instant,
    pub started_at: Instant,
    pub config: serde_json::Value,
    pub stale_threshold: Duration,
}

impl AccountStatus {
    pub fn is_stale(&self) -> bool {
        self.last_event_at.elapsed() > self.stale_threshold
    }
}

pub struct ChannelStatus {
    entries: Mutex<HashMap<String, AccountStatus>>,
}

impl Default for ChannelStatus {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelStatus {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn reset(&self, account_id: &str, config: serde_json::Value, stale_threshold: Duration) {
        let now = Instant::now();
        self.lock().insert(account_id.to_string(), AccountStatus {
            connected: true,
            last_event_at: now,
            started_at: now,
            config,
            stale_threshold,
        });
    }

    pub fn clear(&self, account_id: &str) {
        self.lock().remove(account_id);
    }

    pub fn touch_event(&self, account_id: &str) {
        if let Some(status) = self.lock().get_mut(account_id) {
            status.last_event_at = Instant::now();
        }
    }

    pub fn set_connected(&self, account_id: &str, connected: bool) {
        if let Some(status) = self.lock().get_mut(account_id) {
            status.connected = connected;
            if connected {
                status.last_event_at = Instant::now();
            }
        }
    }

    pub fn get(&self, account_id: &str) -> Option<AccountStatus> {
        self.lock().get(account_id).cloned()
    }

    pub fn default_stale_threshold() -> Duration {
        DEFAULT_STALE_EVENT_THRESHOLD
    }

    fn lock(&self) -> parking_lot::MutexGuard<'_, HashMap<String, AccountStatus>> {
        self.entries.lock()
    }
}
