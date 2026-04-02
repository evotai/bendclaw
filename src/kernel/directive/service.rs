use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use crate::client::DirectiveClient;
use crate::kernel::directive::diagnostics;
use crate::types::Result;

/// Runtime-owned directive cache.
/// Keeps prompt reads off the request path and refreshes in the background.
pub struct DirectiveService {
    client: Arc<DirectiveClient>,
    prompt: RwLock<Option<String>>,
    refresh_interval: Duration,
}

impl DirectiveService {
    pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

    pub fn new(client: Arc<DirectiveClient>, refresh_interval: Duration) -> Self {
        Self {
            client,
            prompt: RwLock::new(None),
            refresh_interval,
        }
    }

    /// Return the latest cached directive snapshot.
    pub fn cached_prompt(&self) -> Option<String> {
        self.prompt.read().clone()
    }

    /// Refresh the cached directive from the platform.
    pub async fn refresh(&self) -> Result<Option<String>> {
        let started = std::time::Instant::now();
        let prompt = self.client.get_directive().await?;
        let mut cache = self.prompt.write();
        let changed = *cache != prompt;
        *cache = prompt.clone();

        diagnostics::log_refresh(
            prompt.as_deref(),
            changed,
            started.elapsed().as_millis() as u64,
        );

        Ok(prompt)
    }

    /// Refresh the directive cache on a fixed interval until cancellation.
    pub fn spawn_refresh_loop(
        self: &Arc<Self>,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        let interval_duration = self.refresh_interval;
        crate::types::spawn_named("directive_refresh_loop", async move {
            let mut interval = tokio::time::interval(interval_duration);
            interval.tick().await;
            diagnostics::log_loop_started(interval_duration.as_millis() as u64);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = service.refresh().await {
                            diagnostics::log_refresh_failed(&e);
                        }
                    }
                    _ = cancel.cancelled() => {

                        break;
                    }
                }
            }
        })
    }
}
