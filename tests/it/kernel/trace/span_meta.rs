use bendclaw::kernel::trace::SpanMeta;
use bendclaw::kernel::Impact;

#[test]
fn span_meta_llm_turn_to_json_contains_iteration() {
    let meta = SpanMeta::LlmTurn { iteration: 7 };
    let json = meta.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["iteration"], 7);
}

#[test]
fn span_meta_llm_result_to_json_contains_finish_reason() {
    let meta = SpanMeta::LlmResult {
        finish_reason: "stop".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json()).unwrap();
    assert_eq!(parsed["finish_reason"], "stop");
}

#[test]
fn span_meta_llm_completed_to_json_contains_tokens() {
    let meta = SpanMeta::LlmCompleted {
        finish_reason: "end_turn".into(),
        prompt_tokens: 200,
        completion_tokens: 80,
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json()).unwrap();
    assert_eq!(parsed["prompt_tokens"], 200);
    assert_eq!(parsed["completion_tokens"], 80);
    assert_eq!(parsed["finish_reason"], "end_turn");
}

#[test]
fn span_meta_llm_failed_to_json_contains_error() {
    let meta = SpanMeta::LlmFailed {
        finish_reason: "error".into(),
        error: "context length exceeded".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json()).unwrap();
    assert_eq!(parsed["error"], "context length exceeded");
    assert_eq!(parsed["finish_reason"], "error");
}

#[test]
fn span_meta_tool_started_to_json_contains_tool_call_id() {
    let meta = SpanMeta::ToolStarted {
        tool_call_id: "call_abc".into(),
        arguments: serde_json::json!({"path": "/tmp"}),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json()).unwrap();
    assert_eq!(parsed["tool_call_id"], "call_abc");
    assert_eq!(parsed["arguments"]["path"], "/tmp");
}

#[test]
fn span_meta_tool_completed_to_json_no_impact_field_when_none() {
    let meta = SpanMeta::ToolCompleted {
        tool_call_id: "call_1".into(),
        duration_ms: 55,
        impact: None,
        summary: "done".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json()).unwrap();
    assert!(parsed.get("impact").is_none());
    assert_eq!(parsed["duration_ms"], 55);
    assert_eq!(parsed["summary"], "done");
}

#[test]
fn span_meta_tool_completed_to_json_has_impact_when_some() {
    let meta = SpanMeta::ToolCompleted {
        tool_call_id: "call_2".into(),
        duration_ms: 10,
        impact: Some(Impact::Medium),
        summary: "wrote file".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json()).unwrap();
    assert_eq!(parsed["impact"], "Medium");
}

#[test]
fn span_meta_tool_failed_to_json_contains_error() {
    let meta = SpanMeta::ToolFailed {
        tool_call_id: "call_3".into(),
        duration_ms: 200,
        error: "permission denied".into(),
        impact: None,
        summary: "write failed".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json()).unwrap();
    assert_eq!(parsed["error"], "permission denied");
    assert_eq!(parsed["tool_call_id"], "call_3");
}

#[test]
fn span_meta_empty_to_json_is_object() {
    let meta = SpanMeta::Empty {};
    let json = meta.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_object());
}

#[test]
fn span_meta_to_json_is_valid_json() {
    let variants: Vec<SpanMeta> = vec![
        SpanMeta::LlmTurn { iteration: 1 },
        SpanMeta::LlmResult {
            finish_reason: "stop".into(),
        },
        SpanMeta::LlmCompleted {
            finish_reason: "end_turn".into(),
            prompt_tokens: 10,
            completion_tokens: 5,
        },
        SpanMeta::LlmFailed {
            finish_reason: "error".into(),
            error: "oops".into(),
        },
        SpanMeta::ToolStarted {
            tool_call_id: "x".into(),
            arguments: serde_json::json!({}),
        },
        SpanMeta::ToolCompleted {
            tool_call_id: "x".into(),
            duration_ms: 1,
            impact: None,
            summary: "ok".into(),
        },
        SpanMeta::ToolFailed {
            tool_call_id: "x".into(),
            duration_ms: 1,
            error: "err".into(),
            impact: None,
            summary: "fail".into(),
        },
        SpanMeta::Empty {},
    ];
    for meta in &variants {
        let json = meta.to_json();
        let result: Result<serde_json::Value, _> = serde_json::from_str(&json);
        assert!(result.is_ok(), "invalid JSON for variant: {json}");
    }
}
