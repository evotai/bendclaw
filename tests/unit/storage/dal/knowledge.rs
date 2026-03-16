use bendclaw::storage::dal::knowledge::build_search_condition;
use bendclaw::storage::dal::knowledge::KnowledgeRecord;

#[test]
fn knowledge_record_serde_roundtrip() {
    let record = KnowledgeRecord {
        id: "k-1".into(),
        kind: "file".into(),
        subject: "file_read".into(),
        locator: "/tmp/test.rs".into(),
        title: "file read success".into(),
        summary: "Read file contents".into(),
        metadata: Some(serde_json::json!({"lines": 42})),
        status: "active".into(),
        confidence: 0.95,
        user_id: "user-1".into(),
        first_run_id: "run-1".into(),
        last_run_id: "run-2".into(),
        first_seen_at: "2026-03-10T00:00:00Z".into(),
        last_seen_at: "2026-03-11T00:00:00Z".into(),
        created_at: "2026-03-10T00:00:00Z".into(),
        updated_at: "2026-03-11T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&record).unwrap();
    let parsed: KnowledgeRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "k-1");
    assert_eq!(parsed.kind, "file");
    assert_eq!(parsed.locator, "/tmp/test.rs");
    assert_eq!(parsed.confidence, 0.95);
    assert_eq!(parsed.metadata.unwrap()["lines"], 42);
}

#[test]
fn knowledge_record_serde_null_metadata() {
    let record = KnowledgeRecord {
        id: "k-2".into(),
        kind: "discovery".into(),
        subject: "web_search".into(),
        locator: String::new(),
        title: "search result".into(),
        summary: "Found something".into(),
        metadata: None,
        status: "active".into(),
        confidence: 1.0,
        user_id: "user-1".into(),
        first_run_id: "run-1".into(),
        last_run_id: "run-1".into(),
        first_seen_at: String::new(),
        last_seen_at: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    let json = serde_json::to_string(&record).unwrap();
    let parsed: KnowledgeRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "k-2");
    assert!(parsed.metadata.is_none());
}

// ── build_search_condition tests ────────────────────

#[test]
fn search_condition_uses_query_not_match() {
    let cond = build_search_condition("hello");
    assert!(cond.starts_with("QUERY("));
    assert!(!cond.contains("MATCH"));
}

#[test]
fn search_condition_multi_column_with_boost() {
    let cond = build_search_condition("test");
    assert!(cond.contains("subject:test^3"));
    assert!(cond.contains("summary:test^2"));
    assert!(cond.contains("locator:test"));
}

#[test]
fn search_condition_escapes_special_chars() {
    let cond = build_search_condition("file:path");
    assert!(cond.contains(r"file\:path"));
    // colon must be escaped so it's not treated as a field separator
    assert!(!cond.contains("file:path^"));
}

#[test]
fn search_condition_escapes_quotes() {
    let cond = build_search_condition("it's a test");
    assert!(cond.contains("its a test"));
}
