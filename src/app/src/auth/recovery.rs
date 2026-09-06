//! Cloud scoped-key recovery at the provider boundary, shared by all hosts.
//! Retry only authentication failures, once, without replaying any tools.
use std::sync::Arc;
use std::sync::OnceLock;

use evot_engine::provider::ProviderError;
use evot_engine::provider::StreamConfig;
use evot_engine::provider::StreamEvent;
use evot_engine::provider::StreamOutcome;
use evot_engine::provider::StreamProvider;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::fetch_catalog;
use super::load_auth;
use super::load_models_cache;
use super::save_models_cache;
use super::CatalogOutcome;
use super::ModelsCache;

pub fn is_scoped_key_error(error: &ProviderError) -> bool {
    matches!(error, ProviderError::Auth(message) if {
        let lower = message.to_ascii_lowercase();
        lower.contains("session_revoked") || (lower.contains("invalid_key") && lower.contains("scoped key"))
    })
}

pub fn with_recovery(
    inner: Arc<dyn StreamProvider>,
    provider: &str,
    model: &evot_engine::provider::ModelConfig,
) -> Arc<dyn StreamProvider> {
    let known = load_models_cache().ok().flatten().is_some_and(|cache| {
        cache.response.providers.iter().any(|entry| {
            entry.name == provider
                && entry.models.iter().any(|id| id == model.id())
                && entry.base_url.trim_end_matches('/') == model.base_url().trim_end_matches('/')
        })
    });
    if !known {
        return inner;
    }
    Arc::new(RecoveringProvider::new(
        inner,
        provider.into(),
        Arc::new(StoredKeyRefresh),
    ))
}

#[async_trait::async_trait]
pub trait KeyRefresh: Send + Sync {
    async fn refresh(&self, provider: &str, config: &StreamConfig)
        -> Result<String, ProviderError>;
}
struct StoredKeyRefresh;
#[async_trait::async_trait]
impl KeyRefresh for StoredKeyRefresh {
    async fn refresh(
        &self,
        provider: &str,
        config: &StreamConfig,
    ) -> Result<String, ProviderError> {
        refreshed_key(provider, config).await
    }
}

pub struct RecoveringProvider {
    inner: Arc<dyn StreamProvider>,
    provider: String,
    refresh: Arc<dyn KeyRefresh>,
}
impl RecoveringProvider {
    pub fn new(
        inner: Arc<dyn StreamProvider>,
        provider: String,
        refresh: Arc<dyn KeyRefresh>,
    ) -> Self {
        Self {
            inner,
            provider,
            refresh,
        }
    }
}

async fn refreshed_key(provider: &str, config: &StreamConfig) -> Result<String, ProviderError> {
    static REFRESH: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = REFRESH.get_or_init(|| Mutex::new(())).lock().await;
    // Sibling evot processes (a spawned subagent, a second TUI) share the same
    // cache file. Serialize across processes so one mint is reused by all.
    let lock_path = super::models_cache_path()
        .map_err(|_| ProviderError::Auth("Cloud credentials could not be read".into()))?
        .with_extension("refresh.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|_| ProviderError::Auth("Cloud credentials could not be read".into()))?;
    let _file_lock =
        tokio::task::spawn_blocking(move || fs2::FileExt::lock_exclusive(&lock).map(|_| lock))
            .await
            .map_err(|_| ProviderError::Auth("Cloud credentials could not be read".into()))?
            .map_err(|_| ProviderError::Auth("Cloud credentials could not be read".into()))?;
    let select = |cache: &ModelsCache| {
        cache
            .response
            .providers
            .iter()
            .find(|entry| {
                entry.name == provider
                    && entry.models.contains(&config.model)
                    && config.model_config.as_ref().is_some_and(|model| {
                        model.base_url().trim_end_matches('/')
                            == entry.base_url.trim_end_matches('/')
                    })
            })
            .map(|entry| entry.api_key.clone())
    };
    if let Some(key) = load_models_cache().ok().flatten().as_ref().and_then(select) {
        if !key.is_empty() && key != config.api_key {
            return Ok(key);
        }
    }
    let state = load_auth()
        .map_err(|_| ProviderError::Auth("Cloud credentials could not be read".into()))?
        .ok_or_else(|| {
            ProviderError::Auth("session_revoked: cloud login required; run /login".into())
        })?;
    match fetch_catalog(&state).await {
        CatalogOutcome::Ready(response) => {
            let cache = ModelsCache::new(evot_engine::now_ms() as i64, response);
            let key = select(&cache)
                .filter(|key| !key.is_empty())
                .ok_or_else(|| {
                    ProviderError::Auth("The selected cloud model is no longer available".into())
                })?;
            let latest = load_auth()
                .map_err(|_| ProviderError::Auth("Cloud credentials could not be read".into()))?;
            if !latest.as_ref().is_some_and(|latest| {
                latest.cli_token == state.cli_token
                    && latest.server_base_url == state.server_base_url
            }) {
                return Err(ProviderError::Auth(
                    "Cloud login changed during refresh; retry with the current login".into(),
                ));
            }
            save_models_cache(&cache).map_err(|_| {
                ProviderError::Auth("Could not save refreshed cloud credentials".into())
            })?;
            Ok(key)
        }
        CatalogOutcome::Refused => Err(ProviderError::Auth(
            // Keep the marker the TUI already recognizes so its /login flow runs.
            "session_revoked: cloud login required; run /login".into(),
        )),
        CatalogOutcome::Unavailable(_) => Err(ProviderError::Auth(
            "Cloud key refresh unavailable; credentials were preserved, retry later".into(),
        )),
    }
}

fn final_auth_error(error: ProviderError) -> ProviderError {
    if is_scoped_key_error(&error) {
        ProviderError::Auth(
            "Refreshed cloud key was rejected; run evot login or retry later".into(),
        )
    } else {
        error
    }
}

#[async_trait::async_trait]
impl StreamProvider for RecoveringProvider {
    async fn stream(
        &self,
        mut config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let result = self
            .inner
            .stream(config.clone(), tx.clone(), cancel.clone())
            .await;
        if result.as_ref().is_err_and(is_scoped_key_error) {
            config.api_key = tokio::select! { _ = cancel.cancelled() => return Err(ProviderError::Cancelled), key = self.refresh.refresh(&self.provider, &config) => key? };
            return self
                .inner
                .stream(config, tx, cancel)
                .await
                .map_err(final_auth_error);
        }
        result
    }
    async fn stream_bounded(
        &self,
        mut config: StreamConfig,
        tx: mpsc::Sender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let result = self
            .inner
            .stream_bounded(config.clone(), tx.clone(), cancel.clone())
            .await;
        if result.as_ref().is_err_and(is_scoped_key_error) {
            config.api_key = tokio::select! { _ = cancel.cancelled() => return Err(ProviderError::Cancelled), key = self.refresh.refresh(&self.provider, &config) => key? };
            return self
                .inner
                .stream_bounded(config, tx, cancel)
                .await
                .map_err(final_auth_error);
        }
        result
    }
}
