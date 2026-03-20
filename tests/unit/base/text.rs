use bendclaw::base::truncate_bytes_on_char_boundary;
use bendclaw::base::truncate_chars_with_ellipsis;
use bendclaw::base::truncate_with_notice;

#[test]
fn truncate_chars_with_ellipsis_preserves_unicode_boundaries() {
    let text = "请分析这个项目，给出详细的改进方案。".repeat(20);
    let truncated = truncate_chars_with_ellipsis(&text, 120);
    assert!(truncated.ends_with("..."));
    assert_eq!(truncated.chars().count(), 120);
}

#[test]
fn truncate_bytes_on_char_boundary_preserves_unicode_boundaries() {
    let text = "abc的def";
    let truncated = truncate_bytes_on_char_boundary(text, 5);
    assert_eq!(truncated, "abc");
}

#[test]
fn truncate_with_notice_returns_original_when_within_limit() {
    let text = "short text";
    let result = truncate_with_notice(text, 100);
    assert_eq!(result, "short text");
}

#[test]
fn truncate_with_notice_truncates_and_appends_notice() {
    let text = "a".repeat(1000);
    let result = truncate_with_notice(&text, 100);
    assert!(result.contains("[truncated: showing "));
    assert!(result.contains("/1000 bytes]"));
    assert!(result.len() < 1000);
}

#[test]
fn truncate_with_notice_preserves_unicode_boundaries() {
    let text = "你好世界".repeat(100);
    let result = truncate_with_notice(&text, 50);
    assert!(result.contains("[truncated:"));
    // Should not panic or produce invalid UTF-8
    assert!(result.is_char_boundary(0));
}
