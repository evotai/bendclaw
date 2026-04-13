use bendengine::tools::validation::truncate_error;
use bendengine::tools::validation::validate_and_coerce;
use serde_json::json;

// ── helper: a typical tool schema ───────────────────────────────────────

fn read_file_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File path" },
            "offset": { "type": "integer", "description": "Start line" },
            "limit": { "type": "integer", "description": "Max lines" }
        },
        "required": ["path"]
    })
}

fn memory_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["add", "replace", "remove", "read"]
            },
            "scope": {
                "type": "string",
                "enum": ["global", "project"]
            },
            "name": { "type": "string" },
            "content": { "type": "string" }
        },
        "required": ["action", "scope"]
    })
}

// ── required fields ─────────────────────────────────────────────────────

#[test]
fn missing_required_param() {
    let input = json!({});
    let err = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap_err();
    assert!(
        err.contains("The required parameter `path` is missing"),
        "got: {err}"
    );
    assert!(err.contains("InputValidationError:"));
    assert!(err.contains("read_file failed"));
}

#[test]
fn missing_multiple_required_params() {
    let input = json!({ "name": "foo" });
    let err = validate_and_coerce("memory", &memory_schema(), &input).unwrap_err();
    assert!(err.contains("`action` is missing"), "got: {err}");
    assert!(err.contains("`scope` is missing"), "got: {err}");
    assert!(err.contains("issues"), "should say 'issues' (plural)");
}

// ── type coercion ───────────────────────────────────────────────────────

#[test]
fn coerce_string_to_integer() {
    let input = json!({ "path": "foo.rs", "offset": "10", "limit": "20" });
    let result = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap();
    assert_eq!(result["offset"], json!(10));
    assert_eq!(result["limit"], json!(20));
    assert_eq!(result["path"], json!("foo.rs"));
}

#[test]
fn coerce_string_to_boolean() {
    let schema = json!({
        "type": "object",
        "properties": {
            "replace_all": { "type": "boolean" }
        },
        "required": ["replace_all"]
    });
    let input = json!({ "replace_all": "true" });
    let result = validate_and_coerce("edit_file", &schema, &input).unwrap();
    assert_eq!(result["replace_all"], json!(true));
}

#[test]
fn coerce_string_to_boolean_case_insensitive() {
    let schema = json!({
        "type": "object",
        "properties": { "flag": { "type": "boolean" } },
        "required": ["flag"]
    });
    for (input_str, expected) in [("TRUE", true), ("False", false), ("TRUE", true)] {
        let input = json!({ "flag": input_str });
        let result = validate_and_coerce("test", &schema, &input).unwrap();
        assert_eq!(result["flag"], json!(expected));
    }
}

#[test]
fn coerce_string_to_array() {
    let schema = json!({
        "type": "object",
        "properties": {
            "items": { "type": "array" }
        },
        "required": ["items"]
    });
    let input = json!({ "items": "[1, 2, 3]" });
    let result = validate_and_coerce("test", &schema, &input).unwrap();
    assert_eq!(result["items"], json!([1, 2, 3]));
}

#[test]
fn coerce_string_to_number() {
    let schema = json!({
        "type": "object",
        "properties": { "score": { "type": "number" } },
        "required": ["score"]
    });
    let input = json!({ "score": "2.5" });
    let result = validate_and_coerce("test", &schema, &input).unwrap();
    assert!((result["score"].as_f64().unwrap() - 2.5).abs() < f64::EPSILON);
}

// ── type mismatch (cannot coerce) ───────────────────────────────────────

#[test]
fn type_mismatch_object_for_string() {
    let input = json!({ "path": { "nested": true } });
    let err = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap_err();
    assert!(
        err.contains("expected as `string` but provided as `object`"),
        "got: {err}"
    );
}

#[test]
fn type_mismatch_string_cannot_parse_as_integer() {
    let input = json!({ "path": "foo.rs", "offset": "not_a_number" });
    let err = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap_err();
    assert!(
        err.contains("expected as `integer` but provided as `string`"),
        "got: {err}"
    );
}

// ── enum validation ─────────────────────────────────────────────────────

#[test]
fn enum_valid_value() {
    let input = json!({ "action": "add", "scope": "global" });
    let result = validate_and_coerce("memory", &memory_schema(), &input).unwrap();
    assert_eq!(result["action"], json!("add"));
}

#[test]
fn enum_invalid_value() {
    let input = json!({ "action": "append", "scope": "global" });
    let err = validate_and_coerce("memory", &memory_schema(), &input).unwrap_err();
    assert!(err.contains("not one of the allowed values"), "got: {err}");
    assert!(err.contains("append"), "got: {err}");
}

#[test]
fn enum_checked_after_coercion() {
    // "1" as string should be coerced to integer 1, then pass enum check.
    let schema = json!({
        "type": "object",
        "properties": {
            "level": { "type": "integer", "enum": [1, 2, 3] }
        },
        "required": ["level"]
    });
    let input = json!({ "level": "1" });
    let result = validate_and_coerce("test", &schema, &input).unwrap();
    assert_eq!(result["level"], json!(1));
}

// ── root input not an object ────────────────────────────────────────────

#[test]
fn root_input_is_string() {
    let input = json!("just a string");
    let err = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap_err();
    assert!(err.contains("must be a JSON object"), "got: {err}");
}

#[test]
fn root_input_is_array() {
    let input = json!([1, 2, 3]);
    let err = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap_err();
    assert!(err.contains("must be a JSON object"), "got: {err}");
}

#[test]
fn root_input_is_null() {
    let input = json!(null);
    let err = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap_err();
    assert!(err.contains("must be a JSON object"), "got: {err}");
}

// ── valid input passes through ──────────────────────────────────────────

#[test]
fn valid_input_passes() {
    let input = json!({ "path": "/tmp/foo.rs", "offset": 10, "limit": 50 });
    let result = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap();
    assert_eq!(result, input);
}

#[test]
fn valid_input_optional_fields_omitted() {
    let input = json!({ "path": "/tmp/foo.rs" });
    let result = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap();
    assert_eq!(result, input);
}

// ── schema without properties (degenerate) ──────────────────────────────

#[test]
fn schema_without_properties_passes() {
    let schema = json!({ "type": "object" });
    let input = json!({ "anything": "goes" });
    let result = validate_and_coerce("test", &schema, &input).unwrap();
    assert_eq!(result, input);
}

// ── truncation ──────────────────────────────────────────────────────────

#[test]
fn truncate_short_error() {
    let short = "short error";
    assert_eq!(truncate_error(short), short);
}

#[test]
fn truncate_long_error() {
    let long = "x".repeat(20_000);
    let result = truncate_error(&long);
    assert!(result.len() < long.len());
    assert!(result.contains("characters truncated"));
    assert!(result.starts_with(&"x".repeat(5_000)));
    assert!(result.ends_with(&"x".repeat(5_000)));
}

#[test]
fn truncate_utf8_safe() {
    // Each '中' is 3 bytes. Build a string that exceeds 10_000 bytes.
    let ch = "中";
    let count = 5_000; // 5000 * 3 = 15_000 bytes
    let long: String = ch.repeat(count);
    let result = truncate_error(&long);
    assert!(result.contains("characters truncated"));
    // Must not panic — the cut landed on a valid char boundary.
    assert!(result.starts_with(ch));
    assert!(result.ends_with(ch));
}

// ── extra fields are preserved (not rejected) ───────────────────────────

#[test]
fn extra_fields_preserved() {
    let input = json!({ "path": "foo.rs", "unknown_field": 42 });
    let result = validate_and_coerce("read_file", &read_file_schema(), &input).unwrap();
    assert_eq!(result["unknown_field"], json!(42));
}

// ── tool result truncation ──────────────────────────────────────────────

use bendengine::tools::validation::truncate_tool_text;
use bendengine::tools::validation::MAX_TOOL_RESULT_BYTES;

#[test]
fn tool_text_within_limit_unchanged() {
    let text = "short output";
    assert_eq!(truncate_tool_text(text, MAX_TOOL_RESULT_BYTES), text);
}

#[test]
fn tool_text_exceeding_limit_truncated() {
    let big = "x".repeat(60_000);
    let result = truncate_tool_text(&big, MAX_TOOL_RESULT_BYTES);
    assert!(result.len() < big.len());
    assert!(result.contains("bytes truncated"));
}

#[test]
fn tool_text_truncation_utf8_safe() {
    let big: String = "中".repeat(20_000); // 60_000 bytes
    let result = truncate_tool_text(&big, MAX_TOOL_RESULT_BYTES);
    assert!(result.contains("bytes truncated"));
    assert!(result.starts_with("中"));
    assert!(result.ends_with("中"));
}

// ── multi-block tool result capping ─────────────────────────────────────

use bendengine::tools::validation::cap_tool_result_content;
use bendengine::types::Content;

#[test]
fn cap_single_block_within_limit_unchanged() {
    let content = vec![Content::Text {
        text: "short".into(),
    }];
    let result = cap_tool_result_content(content.clone(), MAX_TOOL_RESULT_BYTES);
    assert_eq!(result.len(), 1);
    if let Content::Text { text } = &result[0] {
        assert_eq!(text, "short");
    }
}

#[test]
fn cap_multi_block_within_limit_unchanged() {
    let content = vec![
        Content::Text {
            text: "block1".into(),
        },
        Content::Text {
            text: "block2".into(),
        },
    ];
    let result = cap_tool_result_content(content.clone(), MAX_TOOL_RESULT_BYTES);
    // Under limit — blocks preserved as-is
    assert_eq!(result.len(), 2);
}

#[test]
fn cap_multi_block_exceeding_limit_merged_and_truncated() {
    // 10 blocks × 29KB each = 290KB total, well over 30KB limit
    let block = "x".repeat(29_000);
    let content: Vec<Content> = (0..10)
        .map(|_| Content::Text {
            text: block.clone(),
        })
        .collect();
    let result = cap_tool_result_content(content, MAX_TOOL_RESULT_BYTES);

    // Should be merged into a single text block + truncated
    let text_blocks: Vec<&str> = result
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text_blocks.len(), 1, "should merge into one text block");
    assert!(
        text_blocks[0].len() <= MAX_TOOL_RESULT_BYTES + 200, // allow for truncation note
        "merged block should be capped near MAX_TOOL_RESULT_BYTES"
    );
    assert!(text_blocks[0].contains("bytes truncated"));
}

#[test]
fn cap_preserves_non_text_content() {
    // Mix of text (oversized) and non-text — order must be preserved.
    // Original: [Text(big), Image, Text(big)]
    let big_text = Content::Text {
        text: "x".repeat(60_000),
    };
    let image = Content::Image {
        data: "base64data".into(),
        mime_type: "image/png".into(),
    };
    let big_text2 = Content::Text {
        text: "y".repeat(60_000),
    };
    let content = vec![big_text, image, big_text2];
    let result = cap_tool_result_content(content, MAX_TOOL_RESULT_BYTES);

    // Merged text block should appear first (position of first text block),
    // then image stays in its original relative position.
    assert!(
        matches!(&result[0], Content::Text { .. }),
        "first element should be the merged text block"
    );
    assert!(
        matches!(&result[1], Content::Image { .. }),
        "image should stay in its original position after the first text"
    );
    // Only 2 elements: merged text + image (second text block was merged in)
    assert_eq!(result.len(), 2, "should have merged text + image");

    // Text should be truncated
    if let Content::Text { text } = &result[0] {
        assert!(text.contains("bytes truncated"));
    }
}
