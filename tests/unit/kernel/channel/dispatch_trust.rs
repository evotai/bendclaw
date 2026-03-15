use bendclaw::kernel::channel::dispatch::is_sender_allowed;
use serde_json::json;

#[test]
fn allow_all_when_no_allow_from() {
    let config = json!({"token": "abc"});
    assert!(is_sender_allowed(&config, "anyone"));
}

#[test]
fn allow_all_when_empty_list() {
    let config = json!({"allow_from": []});
    assert!(is_sender_allowed(&config, "anyone"));
}

#[test]
fn allow_all_with_wildcard() {
    let config = json!({"allow_from": ["*"]});
    assert!(is_sender_allowed(&config, "anyone"));
}

#[test]
fn allow_matching_sender_id() {
    let config = json!({"allow_from": ["user-1", "user-2"]});
    assert!(is_sender_allowed(&config, "user-1"));
    assert!(is_sender_allowed(&config, "user-2"));
    assert!(!is_sender_allowed(&config, "user-3"));
}

#[test]
fn allow_pipe_separated_format() {
    // Telegram uses "user_id|username" format.
    let config = json!({"allow_from": ["123|alice", "456|bob"]});
    assert!(is_sender_allowed(&config, "123"));
    assert!(is_sender_allowed(&config, "alice"));
    assert!(is_sender_allowed(&config, "456"));
    assert!(is_sender_allowed(&config, "bob"));
    assert!(!is_sender_allowed(&config, "eve"));
}

#[test]
fn reject_unknown_sender() {
    let config = json!({"allow_from": ["trusted-1"]});
    assert!(!is_sender_allowed(&config, "untrusted"));
}
