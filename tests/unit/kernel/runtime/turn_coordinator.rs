use bendclaw::kernel::runtime::explicit_decision_name;
use bendclaw::kernel::runtime::merge_followup;
use bendclaw::kernel::runtime::parse_decision_json;
use bendclaw::kernel::runtime::parse_relation_json;

#[test]
fn parses_plain_relation_json() {
    let (relation, assistant_message) =
        parse_relation_json(r#"{"relation":"revise","assistant_message":"switching"}"#).unwrap();
    assert_eq!(relation, "revise");
    assert_eq!(assistant_message.as_deref(), Some("switching"));
}

#[test]
fn parses_fenced_decision_json() {
    let (decision, assistant_message) = parse_decision_json(
        "```json\n{\"decision\":\"cancel_and_switch\",\"assistant_message\":\"switching now\"}\n```",
    )
    .unwrap();
    assert_eq!(decision, "cancel_and_switch");
    assert_eq!(assistant_message.as_deref(), Some("switching now"));
}

#[test]
fn merges_followup_text() {
    assert_eq!(
        merge_followup(Some("first".to_string()), "second"),
        "first\n\nsecond"
    );
}

#[test]
fn resolves_explicit_decision_reply() {
    assert_eq!(explicit_decision_name("switch"), Some("cancel_and_switch"));
    assert_eq!(explicit_decision_name("continue"), Some("continue_current"));
    assert_eq!(
        explicit_decision_name("followup"),
        Some("append_as_followup")
    );
    assert_eq!(explicit_decision_name("something else"), None);
}
