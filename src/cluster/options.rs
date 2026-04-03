use std::time::Duration;

/// Tunable runtime timings for cluster coordination.
#[derive(Debug, Clone, Copy)]
pub struct ClusterOptions {
    pub heartbeat_interval: Duration,
    pub dispatch_poll_interval: Duration,
}

impl Default for ClusterOptions {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(30),
            dispatch_poll_interval: Duration::from_secs(2),
        }
    }
}
