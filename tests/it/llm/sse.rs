use anyhow::Result;
use bendclaw::llm::sse::SseData;
use bendclaw::llm::sse::SseParser;

#[test]
fn test_parses_single_data_line() -> Result<()> {
    let mut parser = SseParser::new();
    let results = parser.feed(b"data: {\"type\":\"ping\"}\n");
    assert_eq!(results.len(), 1);
    assert!(matches!(&results[0], SseData::Json(v) if v["type"] == "ping"));
    Ok(())
}

#[test]
fn test_parses_done_sentinel() -> Result<()> {
    let mut parser = SseParser::new();
    let results = parser.feed(b"data: [DONE]\n");
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0], SseData::Done));
    Ok(())
}

#[test]
fn test_skips_comments_and_empty_lines() -> Result<()> {
    let mut parser = SseParser::new();
    let results = parser.feed(b": comment\n\ndata: {\"ok\":true}\n");
    assert_eq!(results.len(), 1);
    Ok(())
}

#[test]
fn test_buffers_partial_lines() -> Result<()> {
    let mut parser = SseParser::new();
    let r1 = parser.feed(b"data: {\"par");
    assert!(r1.is_empty());
    let r2 = parser.feed(b"tial\":true}\n");
    assert_eq!(r2.len(), 1);
    assert!(matches!(&r2[0], SseData::Json(v) if v["partial"] == true));
    Ok(())
}

#[test]
fn test_handles_event_lines() -> Result<()> {
    let mut parser = SseParser::new();
    // Anthropic sends "event: message_start" before "data: {...}"
    let results = parser.feed(b"event: message_start\ndata: {\"type\":\"message_start\"}\n");
    assert_eq!(results.len(), 1);
    Ok(())
}

#[test]
fn test_multiple_events_in_one_chunk() -> Result<()> {
    let mut parser = SseParser::new();
    let results = parser.feed(b"data: {\"a\":1}\ndata: {\"b\":2}\n");
    assert_eq!(results.len(), 2);
    Ok(())
}

#[test]
fn test_invalid_json_line_is_ignored() -> Result<()> {
    let mut parser = SseParser::new();
    let results = parser.feed(b"data: {invalid json}\n");
    assert!(results.is_empty());
    Ok(())
}

#[test]
fn test_no_newline_does_not_emit_event() -> Result<()> {
    let mut parser = SseParser::new();
    let results = parser.feed(b"data: {\"x\":1}");
    assert!(results.is_empty());
    let results = parser.feed(b"\n");
    assert_eq!(results.len(), 1);
    assert!(matches!(&results[0], SseData::Json(v) if v["x"] == 1));
    Ok(())
}
