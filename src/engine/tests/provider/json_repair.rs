use bendengine::provider::json_repair::try_repair_json;

#[test]
fn valid_json_passes_through() {
    let v = try_repair_json(r#"{"command":"ls"}"#).unwrap();
    assert_eq!(v["command"], "ls");
}

#[test]
fn trailing_comma_repaired() {
    let v = try_repair_json(r#"{"command": "ls",}"#).unwrap();
    assert_eq!(v["command"], "ls");
}

#[test]
fn truncated_object_repaired() {
    let v = try_repair_json(r#"{"command": "ls""#).unwrap();
    assert_eq!(v["command"], "ls");
}

#[test]
fn plain_text_not_repaired() {
    assert!(try_repair_json("hello world").is_err());
}

#[test]
fn empty_string_not_repaired() {
    assert!(try_repair_json("").is_err());
}

#[test]
fn markdown_fence_not_repaired() {
    assert!(try_repair_json("```json\n{}\n```").is_err());
}

#[test]
fn valid_array_passes_through() {
    let v = try_repair_json(r#"[1, 2, 3]"#).unwrap();
    assert_eq!(v, serde_json::json!([1, 2, 3]));
}

#[test]
fn valid_string_passes_through() {
    let v = try_repair_json(r#""hello""#).unwrap();
    assert_eq!(v, serde_json::json!("hello"));
}
