use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::supervisor::ChannelSupervisor;

pub struct HealthMonitorConfig {
    pub poll_interval: Duration,
    pub restart_cooldown: Duration,
    pub max_restarts: u32,
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(15),
            restart_cooldown: Duration::from_secs(30),
            max_restarts: 5,
        }
    }
}

pub struct ChannelHealthMonitor {
    supervisor: Arc<ChannelSupervisor>,
    config: HealthMonitorConfig,
}

impl ChannelHealthMonitor {
    pub fn new(supervisor: Arc<ChannelSupervisor>, config: HealthMonitorConfig) -> Self {
        Self { supervisor, config }
    }

    /// Single health check pass — public for testability.
    pub async fn check_once(
        &self,
        accounts: &[ChannelAccount],
        restart_counts: &mut HashMap<String, u32>,
        last_restart: &mut HashMap<String, Instant>,
    ) {
        let now = Instant::now();
        for account in accounts {
            let id = &account.channel_account_id;
            if self.supervisor.is_alive(id).await {
                continue;
            }

            let count = restart_counts.entry(id.clone()).or_insert(0);
            if *count >= self.config.max_restarts {
                tracing::error!(
                    channel_account_id = %id,
                    restarts = *count,
                    "health_monitor: max restarts exceeded, giving up"
                );
                continue;
            }

            // Respect cooldown between restarts.
            if let Some(last) = last_restart.get(id) {
                if now.duration_since(*last) < self.config.restart_cooldown {
                    continue;
                }
            }

            tracing::warn!(
                channel_account_id = %id,
                attempt = *count + 1,
                "health_monitor: receiver dead, restarting"
            );

            match self.supervisor.start(account).await {
                Ok(()) => {
                    *count += 1;
                    last_restart.insert(id.clone(), now);
                }
                Err(e) => {
                    tracing::error!(
                        channel_account_id = %id,
                        error = %e,
                        "health_monitor: restart failed"
                    );
                    *count += 1;
                    last_restart.insert(id.clone(), now);
                }
            }
        }
    }

    /// Spawn a background health-check loop.
    pub fn spawn(
        self: Arc<Self>,
        accounts: Vec<ChannelAccount>,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut restart_counts: HashMap<String, u32> = HashMap::new();
            let mut last_restart: HashMap<String, Instant> = HashMap::new();

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("health_monitor: cancelled");
                        return;
                    }
                    _ = tokio::time::sleep(self.config.poll_interval) => {
                        self.check_once(&accounts, &mut restart_counts, &mut last_restart).await;
                    }
                }
            }
        })
    }
}
