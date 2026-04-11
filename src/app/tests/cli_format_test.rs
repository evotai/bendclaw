use bendclaw::agent::RunEvent;
use bendclaw::agent::RunEventPayload;
use bendclaw::cli::format::mask_run_event_for_display;
use bendclaw::cli::format::mask_secrets;
use bendclaw::cli::format::mask_value;
use bendclaw::cli::repl::render::truncate_head_tail;

#[test]
fn short_string_unchanged() {
    assert_eq!(truncate_head_tail("hello world", 56), "hello world");
}

#[test]
fn exact_boundary_unchanged() {
    let s = "a".repeat(56);
    assert_eq!(truncate_head_tail(&s, 56), s);
}

#[test]
fn long_string_shows_head_and_tail() {
    let s = "implement the new session title truncation with head tail display mode";
    let result = truncate_head_tail(s, 40);
    assert!(
        result.contains(" ... "),
        "should contain separator: {result}"
    );
    assert!(
        result.starts_with("implement"),
        "should keep head: {result}"
    );
    assert!(
        result.ends_with("display mode"),
        "should keep tail: {result}"
    );
    assert!(
        result.chars().count() <= 40,
        "should respect max: {} chars in {result}",
        result.chars().count()
    );
}

#[test]
fn unicode_chars_handled() {
    let s = "会话标题截断测试：这是一个很长的中文标题用来验证Unicode字符的正确处理能力";
    let result = truncate_head_tail(s, 24);
    assert!(
        result.contains(" ... "),
        "should contain separator: {result}"
    );
    assert!(
        result.chars().count() <= 24,
        "should respect max: {} chars in {result}",
        result.chars().count()
    );
}

#[test]
fn very_small_max_falls_back_to_plain_truncate() {
    let s = "a]short but still needs truncation";
    let result = truncate_head_tail(s, 10);
    // max < sep_len + 6 = 11, so falls back to plain truncate
    assert!(result.ends_with("..."), "should fall back: {result}");
    assert!(
        !result.contains(" ... "),
        "should not use head-tail: {result}"
    );
}

// ---------------------------------------------------------------------------
// mask_value
// ---------------------------------------------------------------------------

#[test]
fn mask_value_long_string() {
    // "secret-token-123" → "se************23"
    let result = mask_value("secret-token-123");
    assert!(result.starts_with("se"), "should keep first 2: {result}");
    assert!(result.ends_with("23"), "should keep last 2: {result}");
    assert_eq!(result.len(), 16); // same char count as input
    assert!(result.contains('*'), "should contain mask chars: {result}");
}

#[test]
fn mask_value_short_fully_masked() {
    assert_eq!(mask_value("abc"), "***");
    assert_eq!(mask_value("ab"), "**");
    assert_eq!(mask_value("a"), "*");
    assert_eq!(mask_value(""), "");
}

#[test]
fn mask_value_boundary_five_chars() {
    // 5 chars → fully masked
    assert_eq!(mask_value("12345"), "*****");
}

#[test]
fn mask_value_six_chars_shows_edges() {
    let result = mask_value("abcdef");
    assert_eq!(result, "ab**ef");
}

#[test]
fn mask_value_unicode() {
    let result = mask_value("密码是很长的秘密值");
    assert!(result.starts_with("密码"), "should keep first 2: {result}");
    assert!(result.ends_with("密值"), "should keep last 2: {result}");
    let star_count = result.chars().filter(|c| *c == '*').count();
    assert_eq!(star_count, 5, "middle should be masked: {result}");
}

// ---------------------------------------------------------------------------
// mask_secrets
// ---------------------------------------------------------------------------

#[test]
fn mask_secrets_empty_secrets_is_noop() {
    assert_eq!(mask_secrets("hello world", &[]), "hello world");
}

#[test]
fn mask_secrets_replaces_value_in_text() {
    let secrets = vec!["secret-token".to_string()];
    let result = mask_secrets("got secret-token from server", &secrets);
    assert!(
        !result.contains("secret-token"),
        "should be masked: {result}"
    );
    assert!(
        result.contains("se********en"),
        "should contain masked form: {result}"
    );
}

#[test]
fn mask_secrets_longer_secret_replaced_first() {
    // "abcd1234" contains "1234" as a substring.
    // If we replace "1234" first, "abcd1234" won't match anymore.
    // Sorting by length descending ensures the longer one is replaced first.
    let secrets = vec!["1234".to_string(), "abcd1234".to_string()];
    let result = mask_secrets("value is abcd1234 here", &secrets);
    // The long secret should be fully masked
    assert!(
        !result.contains("abcd1234"),
        "long secret should be masked: {result}"
    );
    // The masked form of "abcd1234" is "ab****34" which still contains "34",
    // but the short secret "1234" should not appear in the original text after
    // the long one is replaced.
    assert!(
        !result.contains("1234"),
        "short secret substring should not remain: {result}"
    );
}

#[test]
fn mask_secrets_skips_empty_values() {
    let secrets = vec!["".to_string(), "token".to_string()];
    let result = mask_secrets("my token here", &secrets);
    assert!(!result.contains("token"), "should mask non-empty: {result}");
}

#[test]
fn mask_secrets_deduplicates() {
    let secrets = vec!["abc123".to_string(), "abc123".to_string()];
    let result = mask_secrets("abc123", &secrets);
    // Should not double-mask
    assert_eq!(result, mask_value("abc123"));
}

// ---------------------------------------------------------------------------
// mask_run_event_for_display
// ---------------------------------------------------------------------------

#[test]
fn mask_run_event_for_display_masks_tool_finished_with_escaped_secret() {
    let secret = "ab\"c\\d\nEF".to_string();
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::ToolFinished {
            tool_call_id: "tc-1".into(),
            tool_name: "bash".into(),
            content: format!("stdout: {secret}"),
            is_error: false,
            details: serde_json::Value::Null,
            result_tokens: 3,
            duration_ms: 10,
        },
    );

    let masked = mask_run_event_for_display(&event, std::slice::from_ref(&secret));
    let json = serde_json::to_string(&masked).unwrap();

    assert!(
        !json.contains("ab\\\"c\\\\d\\nEF"),
        "serialized JSON should not contain raw escaped secret: {json}"
    );
    assert!(
        json.contains(&mask_value(&secret)),
        "serialized JSON should contain masked secret: {json}"
    );

    match masked.payload {
        RunEventPayload::ToolFinished { content, .. } => {
            assert_eq!(content, format!("stdout: {}", mask_value(&secret)));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn mask_run_event_for_display_masks_tool_progress_with_escaped_secret() {
    let secret = "xy\"z\\k\nLM".to_string();
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::ToolProgress {
            tool_call_id: "tc-1".into(),
            tool_name: "bash".into(),
            text: format!("progress: {secret}"),
        },
    );

    let masked = mask_run_event_for_display(&event, std::slice::from_ref(&secret));
    let json = serde_json::to_string(&masked).unwrap();

    assert!(
        !json.contains("xy\\\"z\\\\k\\nLM"),
        "serialized JSON should not contain raw escaped secret: {json}"
    );
    assert!(
        json.contains(&mask_value(&secret)),
        "serialized JSON should contain masked secret: {json}"
    );

    match masked.payload {
        RunEventPayload::ToolProgress { text, .. } => {
            assert_eq!(text, format!("progress: {}", mask_value(&secret)));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn mask_run_event_for_display_leaves_other_payloads_unchanged() {
    let event = RunEvent::new("run-1".into(), "sess-1".into(), 1, RunEventPayload::Error {
        message: "oops".into(),
    });

    let masked = mask_run_event_for_display(&event, &["secret".to_string()]);
    match masked.payload {
        RunEventPayload::Error { message } => assert_eq!(message, "oops"),
        _ => panic!("wrong variant"),
    }
}
