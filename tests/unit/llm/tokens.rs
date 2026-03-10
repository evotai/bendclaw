use bendclaw::llm::tokens::count_tokens;

#[test]
fn empty_string_zero_tokens() {
    assert_eq!(count_tokens(""), 0);
}

#[test]
fn single_word() {
    let count = count_tokens("hello");
    assert!(count >= 1);
}

#[test]
fn longer_text_more_tokens() {
    let short = count_tokens("hi");
    let long = count_tokens("This is a much longer sentence with many words in it.");
    assert!(long > short);
}

#[test]
fn whitespace_only() {
    let count = count_tokens("   ");
    assert!(count >= 1);
}
