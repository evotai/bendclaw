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
