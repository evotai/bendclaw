//! Tenant access token cache with proactive refresh.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::base::{ErrorCode, Result};

const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(120);
const DEFAULT_TOKEN_TTL: Duration = Duration::from_secs(7200);
/// Feishu business code for expired/invalid tenant access token.
pub const INVALID_TOKEN_CODE: i64 = 99_991_663;

#[derive(Debug, Clone)]
struct CachedToken {
    value: String,
    refresh_after: Instant,
}

#[derive(Debug, Clone, Default)]
pub struct TokenCache {
    inner: Arc<RwLock<Option<CachedToken>>>,
}

impl TokenCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return cached token if still fresh.
    pub async fn get(&self) -> Option<String> {
        let guard = self.inner.read().await;
        guard.as_ref().and_then(|t| {
            if Instant::now() < t.refresh_after {
                Some(t.value.clone())
            } else {
                None
            }
        })
    }

    /// Store a new token with its TTL.
    pub async fn set(&self, value: String, ttl_secs: u64) {
        let ttl = Duration::from_secs(ttl_secs.max(1));
        let refresh_after = Instant::now()
            + ttl.checked_sub(TOKEN_REFRESH_SKEW).unwrap_or(Duration::from_secs(1));
        let mut guard = self.inner.write().await;
        *guard = Some(CachedToken { value, refresh_after });
    }

    /// Invalidate cached token (called on 401 or business code 99991663).
    pub async fn invalidate(&self) {
        let mut guard = self.inner.write().await;
        *guard = None;
    }
}

/// Fetch a fresh tenant access token from Feishu API.
pub async fn fetch_token(
    client: &reqwest::Client,
    api_base: &str,
    app_id: &str,
    app_secret: &str,
) -> Result<(String, u64)> {
    let url = format!("{api_base}/auth/v3/tenant_access_token/internal");
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "app_id": app_id, "app_secret": app_secret }))
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
        .ok_or_else(|| ErrorCode::internal("feishu: missing tenant_access_token"))?
        .to_string();

    let ttl = json["expire"]
        .as_u64()
        .unwrap_or(DEFAULT_TOKEN_TTL.as_secs());

    Ok((token, ttl))
}

/// Get a valid token, using cache and fetching only when needed.
pub async fn get_token(
    client: &reqwest::Client,
    api_base: &str,
    app_id: &str,
    app_secret: &str,
    cache: &TokenCache,
) -> Result<String> {
    if let Some(token) = cache.get().await {
        return Ok(token);
    }
    let (token, ttl) = fetch_token(client, api_base, app_id, app_secret).await?;
    cache.set(token.clone(), ttl).await;
    Ok(token)
}

/// Returns true when the response indicates the token is invalid and should be refreshed.
pub fn is_token_error(status: reqwest::StatusCode, body: &serde_json::Value) -> bool {
    status == reqwest::StatusCode::UNAUTHORIZED
        || body["code"].as_i64() == Some(INVALID_TOKEN_CODE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_miss_on_empty() {
        let cache = TokenCache::new();
        assert!(cache.get().await.is_none());
    }

    #[tokio::test]
    async fn cache_hit_after_set() {
        let cache = TokenCache::new();
        cache.set("tok123".into(), 7200).await;
        assert_eq!(cache.get().await.as_deref(), Some("tok123"));
    }

    #[tokio::test]
    async fn cache_miss_after_invalidate() {
        let cache = TokenCache::new();
        cache.set("tok123".into(), 7200).await;
        cache.invalidate().await;
        assert!(cache.get().await.is_none());
    }

    #[tokio::test]
    async fn cache_miss_when_expired() {
        let cache = TokenCache::new();
        // Write a token whose refresh_after is already in the past
        {
            let mut guard = cache.inner.write().await;
            *guard = Some(CachedToken {
                value: "tok".into(),
                refresh_after: std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(1))
                    .unwrap_or(std::time::Instant::now()),
            });
        }
        assert!(cache.get().await.is_none());
    }

    #[test]
    fn is_token_error_on_401() {
        let body = serde_json::json!({ "code": 0 });
        assert!(is_token_error(reqwest::StatusCode::UNAUTHORIZED, &body));
    }

    #[test]
    fn is_token_error_on_business_code() {
        let body = serde_json::json!({ "code": INVALID_TOKEN_CODE });
        assert!(is_token_error(reqwest::StatusCode::OK, &body));
    }

    #[test]
    fn is_token_error_false_on_success() {
        let body = serde_json::json!({ "code": 0 });
        assert!(!is_token_error(reqwest::StatusCode::OK, &body));
    }
}
