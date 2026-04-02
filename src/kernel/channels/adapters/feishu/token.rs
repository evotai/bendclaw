use std::sync::Arc;

use tokio::sync::RwLock;

use super::config::FEISHU_API;
use crate::base::ErrorCode;
use crate::base::Result;

// ── TokenCache ──

struct CachedToken {
    value: String,
    expires_at: tokio::time::Instant,
}

/// Thread-safe tenant access token cache with TTL.
#[derive(Clone)]
pub struct TokenCache {
    inner: Arc<RwLock<Option<CachedToken>>>,
}

impl Default for TokenCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn get(&self) -> Option<String> {
        let guard = self.inner.read().await;
        guard.as_ref().and_then(|c| {
            if tokio::time::Instant::now() < c.expires_at {
                Some(c.value.clone())
            } else {
                None
            }
        })
    }

    pub async fn set(&self, value: String, ttl_secs: u64) {
        // Safety margin: expire 120s early (official SDK uses 60s)
        let effective_ttl = ttl_secs.saturating_sub(120).max(60);
        let expires_at =
            tokio::time::Instant::now() + std::time::Duration::from_secs(effective_ttl);
        let mut guard = self.inner.write().await;
        *guard = Some(CachedToken { value, expires_at });
    }

    pub async fn invalidate(&self) {
        let mut guard = self.inner.write().await;
        *guard = None;
    }
}

// ── Token fetch ──

/// Fetch a fresh tenant access token from Feishu API.
pub async fn fetch_token(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
) -> Result<(String, u64)> {
    let url = format!("{FEISHU_API}/auth/v3/tenant_access_token/internal");
    let body = serde_json::json!({
        "app_id": app_id,
        "app_secret": app_secret,
    });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu auth: {e}")))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu auth response: {e}")))?;

    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["msg"].as_str().unwrap_or("unknown");
        return Err(ErrorCode::internal(format!(
            "feishu auth failed: code={code}, msg={msg}"
        )));
    }

    let token = json["tenant_access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            ErrorCode::internal(format!(
                "feishu: missing tenant_access_token in response: {json}"
            ))
        })?;

    let expire = json["expire"].as_u64().unwrap_or(7200);
    Ok((token, expire))
}

/// Get token from cache, or fetch and cache a new one.
pub async fn get_token(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
    cache: &TokenCache,
) -> Result<String> {
    if let Some(token) = cache.get().await {
        return Ok(token);
    }
    let (token, expire) = fetch_token(client, app_id, app_secret).await?;
    cache.set(token.clone(), expire).await;
    Ok(token)
}

/// Check if an HTTP response indicates a token error (expired/invalid).
/// Returns true for HTTP 401 or Feishu error code 99991663 (token invalid).
pub fn is_token_error(status: u16, body: &serde_json::Value) -> bool {
    if status == 401 {
        return true;
    }
    body["code"].as_i64() == Some(99991663)
}
