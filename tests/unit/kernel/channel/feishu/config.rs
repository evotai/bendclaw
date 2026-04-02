use bendclaw::kernel::channels::adapters::feishu::config::is_sender_allowed;
use bendclaw::kernel::channels::adapters::feishu::config::ReconnectConfig;
use bendclaw::kernel::channels::adapters::feishu::FeishuConfig;

#[test]
fn config_deserialize_defaults() {
    let json = serde_json::json!({
        "app_id": "id",
        "app_secret": "secret",
    });
    let c: FeishuConfig = serde_json::from_value(json).unwrap();
    assert!(c.mention_only);
    assert!(c.allow_from.is_empty());
}

#[test]
fn config_mention_only_false() {
    let json = serde_json::json!({
        "app_id": "id",
        "app_secret": "secret",
        "mention_only": false,
    });
    let c: FeishuConfig = serde_json::from_value(json).unwrap();
    assert!(!c.mention_only);
}

#[test]
fn allow_from_empty_allows_all() {
    assert!(is_sender_allowed(&[], "anyone"));
}

#[test]
fn allow_from_wildcard() {
    let list = vec!["*".to_string()];
    assert!(is_sender_allowed(&list, "anyone"));
}

#[test]
fn allow_from_specific() {
    let list = vec!["user_a".to_string(), "user_b".to_string()];
    assert!(is_sender_allowed(&list, "user_a"));
    assert!(!is_sender_allowed(&list, "user_c"));
}

#[test]
fn reconnect_config_from_client_config() {
    let json = serde_json::json!({
        "ReconnectCount": 10,
        "ReconnectInterval": 3,
        "ReconnectNonce": 5,
    });
    let rc = ReconnectConfig::from_client_config(&json);
    assert_eq!(rc.reconnect_count, 10);
    assert_eq!(rc.reconnect_interval, 3);
    assert_eq!(rc.reconnect_nonce, 5);
}

#[test]
fn reconnect_config_defaults() {
    let rc = ReconnectConfig::from_client_config(&serde_json::json!({}));
    assert_eq!(rc.reconnect_count, 0);
    assert_eq!(rc.reconnect_interval, 5);
    assert_eq!(rc.reconnect_nonce, 0);
}

#[test]
fn reconnect_config_update_from_pong() {
    let mut rc = ReconnectConfig::default();
    let pong = serde_json::json!({
        "ReconnectCount": 20,
        "ReconnectInterval": 8,
    });
    rc.update_from_pong(&pong);
    assert_eq!(rc.reconnect_count, 20);
    assert_eq!(rc.reconnect_interval, 8);
}
