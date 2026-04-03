use std::collections::HashMap;

use bendclaw::channels::adapters::feishu::ws::decode_frame;
use bendclaw::channels::adapters::feishu::ws::redact_ws_url;
use bendclaw::channels::adapters::feishu::ws::PbFrame;
use bendclaw::channels::adapters::feishu::ws::PbHeader;
use prost::Message as ProstMessage;

const FRAME_METHOD_CONTROL: i32 = 0;
const FRAME_METHOD_DATA: i32 = 1;

fn make_data_frame(msg_type: &str, msg_id: &str, payload: &str) -> Vec<u8> {
    let frame = PbFrame {
        seq_id: 1,
        log_id: 1,
        service: 0,
        method: FRAME_METHOD_DATA,
        headers: vec![
            PbHeader {
                key: "type".into(),
                value: msg_type.into(),
            },
            PbHeader {
                key: "message_id".into(),
                value: msg_id.into(),
            },
            PbHeader {
                key: "trace_id".into(),
                value: "trace_1".into(),
            },
            PbHeader {
                key: "sum".into(),
                value: "1".into(),
            },
            PbHeader {
                key: "seq".into(),
                value: "0".into(),
            },
        ],
        payload_encoding: None,
        payload_type: None,
        payload: Some(payload.as_bytes().to_vec()),
        log_id_new: None,
    };
    frame.encode_to_vec()
}

fn make_pong_frame(payload: &serde_json::Value) -> Vec<u8> {
    let frame = PbFrame {
        seq_id: 0,
        log_id: 0,
        service: 0,
        method: FRAME_METHOD_CONTROL,
        headers: vec![PbHeader {
            key: "type".into(),
            value: "pong".into(),
        }],
        payload_encoding: None,
        payload_type: None,
        payload: Some(payload.to_string().into_bytes()),
        log_id_new: None,
    };
    frame.encode_to_vec()
}

#[test]
fn decode_event_frame() {
    let mut cache = HashMap::new();
    let data = make_data_frame("event", "msg_1", r#"{"hello":"world"}"#);
    let decoded = decode_frame(&data, &mut cache);
    assert!(decoded.response.is_some());
    assert_eq!(
        decoded.event_payload.as_deref(),
        Some(r#"{"hello":"world"}"#)
    );
    assert!(decoded.updated_ping_interval.is_none());
}
#[test]
fn decode_non_event_data_frame() {
    let mut cache = HashMap::new();
    let data = make_data_frame("other", "msg_2", "payload");
    let decoded = decode_frame(&data, &mut cache);
    assert!(decoded.response.is_some());
    assert!(decoded.event_payload.is_none());
}

#[test]
fn decode_pong_updates_ping_interval() {
    let mut cache = HashMap::new();
    let data = make_pong_frame(&serde_json::json!({"PingInterval": 60}));
    let decoded = decode_frame(&data, &mut cache);
    assert!(decoded.response.is_none());
    assert_eq!(
        decoded.updated_ping_interval,
        Some(std::time::Duration::from_secs(60))
    );
}

#[test]
fn decode_pong_updates_reconnect_config() {
    let mut cache = HashMap::new();
    let data = make_pong_frame(&serde_json::json!({
        "PingInterval": 90,
        "ReconnectCount": 5,
        "ReconnectInterval": 10,
    }));
    let decoded = decode_frame(&data, &mut cache);
    let rc = decoded.updated_reconnect_config.unwrap();
    assert_eq!(rc.reconnect_count, 5);
    assert_eq!(rc.reconnect_interval, 10);
}

#[test]
fn decode_invalid_data() {
    let mut cache = HashMap::new();
    let decoded = decode_frame(b"not valid protobuf", &mut cache);
    assert!(decoded.response.is_none());
    assert!(decoded.event_payload.is_none());
}

#[test]
fn decode_multipart_message() {
    let mut cache = HashMap::new();

    // Part 1 of 2
    let frame1 = PbFrame {
        seq_id: 1,
        log_id: 1,
        service: 0,
        method: FRAME_METHOD_DATA,
        headers: vec![
            PbHeader {
                key: "type".into(),
                value: "event".into(),
            },
            PbHeader {
                key: "message_id".into(),
                value: "multi_1".into(),
            },
            PbHeader {
                key: "trace_id".into(),
                value: "t".into(),
            },
            PbHeader {
                key: "sum".into(),
                value: "2".into(),
            },
            PbHeader {
                key: "seq".into(),
                value: "0".into(),
            },
        ],
        payload_encoding: None,
        payload_type: None,
        payload: Some(b"hello ".to_vec()),
        log_id_new: None,
    };
    let decoded1 = decode_frame(&frame1.encode_to_vec(), &mut cache);
    assert!(decoded1.event_payload.is_none()); // incomplete

    // Part 2 of 2
    let frame2 = PbFrame {
        seq_id: 2,
        log_id: 1,
        service: 0,
        method: FRAME_METHOD_DATA,
        headers: vec![
            PbHeader {
                key: "type".into(),
                value: "event".into(),
            },
            PbHeader {
                key: "message_id".into(),
                value: "multi_1".into(),
            },
            PbHeader {
                key: "trace_id".into(),
                value: "t".into(),
            },
            PbHeader {
                key: "sum".into(),
                value: "2".into(),
            },
            PbHeader {
                key: "seq".into(),
                value: "1".into(),
            },
        ],
        payload_encoding: None,
        payload_type: None,
        payload: Some(b"world".to_vec()),
        log_id_new: None,
    };
    let decoded2 = decode_frame(&frame2.encode_to_vec(), &mut cache);
    assert_eq!(decoded2.event_payload.as_deref(), Some("hello world"));
    assert!(cache.is_empty()); // cleaned up after combining
}

#[test]
fn redact_ws_url_hides_secrets() {
    let url = "wss://example.com/ws?access_key=secret123&ticket=tok456&service_id=7";
    let redacted = redact_ws_url(url);
    assert!(redacted.contains("access_key=***"));
    assert!(redacted.contains("ticket=***"));
    assert!(redacted.contains("service_id=7"));
    assert!(!redacted.contains("secret123"));
}

#[test]
fn redact_ws_url_invalid_url() {
    let url = "not a url";
    assert_eq!(redact_ws_url(url), url);
}
