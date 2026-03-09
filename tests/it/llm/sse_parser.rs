use bendclaw::llm::sse::SseData;
use bendclaw::llm::sse::SseParser;

#[test]
fn parse_json_data() {
    let mut parser = SseParser::new();
    let results = parser.feed(b"data: {\"key\":\"value\"}\n\n");
    assert_eq!(results.len(), 1);
    match &results[0] {
        SseData::Json(v) => assert_eq!(v["key"], "value"),
        _ => panic!("expected Json"),
    }
}

#[test]
fn parse_done_sentinel() {
    let mut parser = SseParser::new();
    let results = parser.feed(b"data: [DONE]\n\n");
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0], SseData::Done));
}

#[test]
fn skip_comments() {
    let mut parser = SseParser::new();
    let results = parser.feed(b": this is a comment\ndata: {\"a\":1}\n\n");
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0], SseData::Json(_)));
}

#[test]
fn skip_empty_lines() {
    let mut parser = SseParser::new();
    let results = parser.feed(b"\n\ndata: {\"x\":1}\n\n");
    assert_eq!(results.len(), 1);
}

#[test]
fn skip_non_data_lines() {
    let mut parser = SseParser::new();
    let results = parser.feed(b"event: message\ndata: {\"a\":1}\n\n");
    assert_eq!(results.len(), 1);
}

#[test]
fn partial_chunks_buffered() {
    let mut parser = SseParser::new();
    let r1 = parser.feed(b"data: {\"ke");
    assert!(r1.is_empty());
    let r2 = parser.feed(b"y\":\"val\"}\n\n");
    assert_eq!(r2.len(), 1);
}

#[test]
fn multiple_events_in_one_chunk() {
    let mut parser = SseParser::new();
    let results = parser.feed(b"data: {\"a\":1}\ndata: {\"b\":2}\n\n");
    assert_eq!(results.len(), 2);
}

#[test]
fn invalid_json_skipped() {
    let mut parser = SseParser::new();
    let results = parser.feed(b"data: not-json\ndata: {\"ok\":true}\n\n");
    assert_eq!(results.len(), 1);
    match &results[0] {
        SseData::Json(v) => assert_eq!(v["ok"], true),
        _ => panic!("expected Json"),
    }
}

#[test]
fn default_creates_new() {
    let parser = SseParser::default();
    let _ = parser;
}
