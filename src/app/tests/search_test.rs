use evot::search::SessionSearcher;
use evot::search::TextMatcher;
use evot::types::SessionMeta;
use evot::types::TranscriptEntry;
use evot::types::TranscriptItem;

#[test]
fn text_matcher_empty_matches_everything() {
    let m = TextMatcher::new("");
    assert!(m.matches("anything"));
    assert!(m.matches(""));
}

#[test]
fn text_matcher_substring() {
    let m = TextMatcher::new("deploy");
    assert!(m.matches("auto-deploy-service"));
    assert!(!m.matches("data pipeline"));
}

#[test]
fn text_matcher_case_insensitive() {
    let m = TextMatcher::new("Deploy");
    assert!(m.matches("auto-deploy-service"));
    assert!(m.matches("DEPLOY_NOW"));
}

#[test]
fn text_matcher_subsequence() {
    let m = TextMatcher::new("dpl");
    assert!(m.matches("deploy"));
    assert!(!m.matches("apple"));
}

#[test]
fn text_matcher_subsequence_order_matters() {
    let m = TextMatcher::new("abc");
    assert!(m.matches("a_b_c"));
    assert!(!m.matches("c_b_a"));
}

fn make_session(id: &str, title: &str, cwd: &str) -> SessionMeta {
    let mut s = SessionMeta::new(id.to_string(), cwd.to_string(), "test-model".to_string());
    s.title = Some(title.to_string());
    s
}

fn make_entry(session_id: &str, seq: u64, item: TranscriptItem) -> TranscriptEntry {
    TranscriptEntry::new(session_id.to_string(), None, seq, 1, item)
}

#[test]
fn searcher_matches_title() {
    let session = make_session("s1", "databend doc migration", "/home/user/project");
    let searcher = SessionSearcher::new("databend");
    let hit = searcher.matches_meta(&session);
    assert!(hit.is_some());
    assert_eq!(hit.as_ref().unwrap().matched_field, "title");
}

#[test]
fn searcher_matches_cwd() {
    let session = make_session("s1", "untitled", "/home/user/databend");
    let searcher = SessionSearcher::new("databend");
    let hit = searcher.matches_meta(&session);
    assert!(hit.is_some());
    assert_eq!(hit.as_ref().unwrap().matched_field, "cwd");
}

#[test]
fn searcher_no_meta_match() {
    let session = make_session("s1", "fix login bug", "/home/user/webapp");
    let searcher = SessionSearcher::new("databend");
    assert!(searcher.matches_meta(&session).is_none());
}

#[test]
fn searcher_matches_transcript_user_text() {
    let session = make_session("s1", "untitled", "/tmp");
    let entries = vec![make_entry("s1", 1, TranscriptItem::User {
        text: "help me with databend documentation".to_string(),
        content: vec![],
    })];
    let searcher = SessionSearcher::new("databend");
    let hit = searcher.matches_transcript(&session, &entries);
    assert!(hit.is_some());
    assert_eq!(hit.as_ref().unwrap().matched_field, "content");
}

#[test]
fn searcher_matches_transcript_assistant_text() {
    let session = make_session("s1", "untitled", "/tmp");
    let entries = vec![make_entry("s1", 1, TranscriptItem::Assistant {
        text: "Here is the databend query syntax".to_string(),
        thinking: None,
        tool_calls: vec![],
        stop_reason: "end_turn".to_string(),
    })];
    let searcher = SessionSearcher::new("databend");
    let hit = searcher.matches_transcript(&session, &entries);
    assert!(hit.is_some());
}

#[test]
fn searcher_empty_query_matches_all_meta() {
    let session = make_session("s1", "anything", "/tmp");
    let searcher = SessionSearcher::new("");
    assert!(searcher.matches_meta(&session).is_some());
}

#[test]
fn searcher_empty_query_skips_transcript() {
    let session = make_session("s1", "anything", "/tmp");
    let entries = vec![make_entry("s1", 1, TranscriptItem::User {
        text: "hello".to_string(),
        content: vec![],
    })];
    let searcher = SessionSearcher::new("");
    assert!(searcher.matches_transcript(&session, &entries).is_none());
}

#[test]
fn searcher_fuzzy_matches_transcript() {
    let session = make_session("s1", "untitled", "/tmp");
    let entries = vec![make_entry("s1", 1, TranscriptItem::User {
        text: "deploy the service".to_string(),
        content: vec![],
    })];
    let searcher = SessionSearcher::new("dpl");
    let hit = searcher.matches_transcript(&session, &entries);
    assert!(hit.is_some());
}

#[test]
fn searcher_snippet_truncates_utf8_safely() {
    let session = make_session("s1", "untitled", "/tmp");
    let long_cjk =
        "这是一段很长的中文文本用来测试截断功能是否会在多字节字符边界上出错导致panic的情况"
            .to_string();
    let entries = vec![make_entry("s1", 1, TranscriptItem::User {
        text: long_cjk,
        content: vec![],
    })];
    let searcher = SessionSearcher::new("测试");
    let hit = searcher.matches_transcript(&session, &entries);
    assert!(hit.is_some());
    let snippet = &hit.unwrap().snippet;
    assert!(snippet.len() <= 400);
}
