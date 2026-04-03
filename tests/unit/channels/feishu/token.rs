use bendclaw::channels::adapters::feishu::token::is_token_error;
use bendclaw::channels::adapters::feishu::token::TokenCache;

#[tokio::test]
async fn cache_miss_returns_none() {
    let cache = TokenCache::new();
    assert!(cache.get().await.is_none());
}

#[tokio::test]
async fn cache_hit_after_set() {
    let cache = TokenCache::new();
    cache.set("tok123".to_string(), 7200).await;
    assert_eq!(cache.get().await.as_deref(), Some("tok123"));
}

#[tokio::test]
async fn cache_invalidate() {
    let cache = TokenCache::new();
    cache.set("tok123".to_string(), 7200).await;
    cache.invalidate().await;
    assert!(cache.get().await.is_none());
}

#[test]
fn is_token_error_401() {
    assert!(is_token_error(401, &serde_json::json!({})));
}

#[test]
fn is_token_error_code_99991663() {
    assert!(is_token_error(200, &serde_json::json!({"code": 99991663})));
}

#[test]
fn is_token_error_normal_response() {
    assert!(!is_token_error(200, &serde_json::json!({"code": 0})));
}
