use bendclaw::kernel::skills::executor::parse_skill_args;
use bendclaw::kernel::skills::executor::SkillError;
use bendclaw::kernel::skills::executor::SkillOutput;

#[test]
fn skill_output_is_error_true() {
    let out = SkillOutput {
        data: None,
        error: Some("fail".into()),
    };
    assert!(out.is_error());
}

#[test]
fn skill_output_is_error_false() {
    let out = SkillOutput {
        data: Some(serde_json::json!("ok")),
        error: None,
    };
    assert!(!out.is_error());
}

#[test]
fn skill_output_serde_roundtrip() {
    let out = SkillOutput {
        data: Some(serde_json::json!({"key": "value"})),
        error: None,
    };
    let json = serde_json::to_string(&out).unwrap();
    let back: SkillOutput = serde_json::from_str(&json).unwrap();
    assert!(!back.is_error());
    assert!(back.data.is_some());
}

#[test]
fn skill_error_display() {
    let err = SkillError {
        skill_name: "python-runner".into(),
        message: "timeout".into(),
        exit_code: Some(1),
    };
    assert_eq!(err.to_string(), "skill 'python-runner': timeout");
}

#[test]
fn skill_error_serde_roundtrip() {
    let err = SkillError {
        skill_name: "test".into(),
        message: "bad".into(),
        exit_code: None,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: SkillError = serde_json::from_str(&json).unwrap();
    assert_eq!(back.skill_name, "test");
    assert!(back.exit_code.is_none());
}

#[test]
fn parse_skill_args_valid_json_object() {
    let args = parse_skill_args("test", r#"{"name":"hello","count":3}"#);
    assert!(args.contains(&"--name".to_string()));
    assert!(args.contains(&"hello".to_string()));
    assert!(args.contains(&"--count".to_string()));
    assert!(args.contains(&"3".to_string()));
}

#[test]
fn parse_skill_args_invalid_json() {
    let args = parse_skill_args("test", "not json");
    assert!(args.is_empty());
}

#[test]
fn parse_skill_args_empty_object() {
    let args = parse_skill_args("test", "{}");
    assert!(args.is_empty());
}

#[test]
fn parse_skill_args_string_value() {
    let args = parse_skill_args("test", r#"{"query":"hello world"}"#);
    assert_eq!(args, vec!["--query", "hello world"]);
}
