use bendclaw::kernel::session::core::session_rules::SessionRules;

#[test]
fn empty_rules() {
    let rules = SessionRules::default();
    assert!(rules.is_empty());
}

#[test]
fn rules_with_text() {
    let rules = SessionRules {
        rules_text: "Always use Rust.".into(),
        preferences: vec!["prefer-rust".into()],
        conventions: vec![],
    };
    assert!(!rules.is_empty());
    assert_eq!(rules.rules_text, "Always use Rust.");
}

#[test]
fn rules_serde_roundtrip() {
    let rules = SessionRules {
        rules_text: "test".into(),
        preferences: vec!["a".into(), "b".into()],
        conventions: vec!["c".into()],
    };
    let json = serde_json::to_string(&rules).unwrap();
    let back: SessionRules = serde_json::from_str(&json).unwrap();
    assert_eq!(back.rules_text, "test");
    assert_eq!(back.preferences.len(), 2);
}
