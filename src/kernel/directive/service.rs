use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use crate::base::Result;
use crate::client::DirectiveClient;
use crate::observability::log::slog;

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

        match (&prompt, changed) {
            (Some(text), true) => slog!(
                info,
                "directive",
                "refreshed",
                size = text.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
            ),
            (Some(text), false) => slog!(
                info,
                "directive",
                "unchanged",
                size = text.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
            ),
            (None, true) => slog!(
                info,
                "directive",
                "cleared",
                elapsed_ms = started.elapsed().as_millis() as u64,
            ),
            (None, false) => slog!(
                info,
                "directive",
                "empty",
                elapsed_ms = started.elapsed().as_millis() as u64,
            ),
        }

        Ok(prompt)
    }

    /// Refresh the directive cache on a fixed interval until cancellation.
    pub fn spawn_refresh_loop(
        self: &Arc<Self>,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        let interval_duration = self.refresh_interval;
        crate::base::spawn_named("directive_refresh_loop", async move {
            let mut interval = tokio::time::interval(interval_duration);
            interval.tick().await;
            slog!(
                info,
                "directive",
                "loop_started",
                refresh_interval_ms = interval_duration.as_millis() as u64,
            );
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = service.refresh().await {
                            slog!(warn, "directive", "refresh_failed", error = %e,);
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
