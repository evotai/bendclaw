use bendclaw::app::result::event_envelope::EventEnvelope;
use bendclaw::app::result::formats;

fn sample_envelopes() -> Vec<EventEnvelope> {
    vec![
        EventEnvelope {
            sequence: 1,
            timestamp: "2026-01-01T00:00:00Z".into(),
            session_id: "s01".into(),
            run_id: "r01".into(),
            event_name: "user.input".into(),
            payload: serde_json::json!({"prompt": "hello"}),
            cursor: None,
        },
        EventEnvelope {
            sequence: 2,
            timestamp: "2026-01-01T00:00:01Z".into(),
            session_id: "s01".into(),
            run_id: "r01".into(),
            event_name: "assistant.output".into(),
            payload: serde_json::json!({"text": "world"}),
            cursor: None,
        },
    ]
}

#[test]
fn text_collects_assistant_output() {
    let envs = sample_envelopes();
    let text = formats::text::collect_text(&envs);
    assert_eq!(text, "world");
}

#[test]
fn json_collects_all_events() {
    let envs = sample_envelopes();
    let json = formats::json::collect_json(&envs);
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 2);
}

#[test]
fn stream_json_encodes_single_line() {
    let envs = sample_envelopes();
    let line = formats::stream_json::encode(&envs[0]);
    assert!(!line.contains('\n'));
    let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(parsed["event_name"], "user.input");
}

#[test]
fn sse_encodes_event_format() {
    let envs = sample_envelopes();
    let sse = formats::sse::encode(&envs[1]);
    assert!(sse.starts_with("event: assistant.output\n"));
    assert!(sse.contains("data: "));
    assert!(sse.ends_with("\n\n"));
}
