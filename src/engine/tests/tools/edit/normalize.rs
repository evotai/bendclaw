use bendengine::tools::edit::normalize::*;

#[test]
fn detect_pure_lf() {
    assert_eq!(detect_line_ending("a\nb\nc\n"), LineEnding::Lf);
}

#[test]
fn detect_pure_crlf() {
    assert_eq!(detect_line_ending("a\r\nb\r\nc\r\n"), LineEnding::CrLf);
}

#[test]
fn detect_mixed_majority_crlf() {
    assert_eq!(detect_line_ending("a\r\nb\r\nc\n"), LineEnding::CrLf);
}

#[test]
fn detect_empty_defaults_lf() {
    assert_eq!(detect_line_ending(""), LineEnding::Lf);
}

#[test]
fn normalize_crlf_to_lf() {
    assert_eq!(normalize_to_lf("a\r\nb\r\n"), "a\nb\n");
}

#[test]
fn normalize_bare_cr() {
    assert_eq!(normalize_to_lf("a\rb\r"), "a\nb\n");
}

#[test]
fn restore_to_crlf() {
    assert_eq!(
        restore_line_endings("a\nb\n", LineEnding::CrLf),
        "a\r\nb\r\n"
    );
}

#[test]
fn restore_to_lf_noop() {
    assert_eq!(restore_line_endings("a\nb\n", LineEnding::Lf), "a\nb\n");
}

#[test]
fn strip_bom_present() {
    let input = "\u{FEFF}hello";
    let (bom, content) = strip_utf8_bom(input);
    assert_eq!(bom, "\u{FEFF}");
    assert_eq!(content, "hello");
}

#[test]
fn strip_bom_absent() {
    let (bom, content) = strip_utf8_bom("hello");
    assert_eq!(bom, "");
    assert_eq!(content, "hello");
}

#[test]
fn normalize_curly_quotes() {
    let input = "\u{201C}hello\u{201D} \u{2018}world\u{2019}";
    let result = normalize_quotes(input);
    assert_eq!(result, "\"hello\" 'world'");
    assert_eq!(input.chars().count(), result.chars().count());
}

#[test]
fn normalize_quotes_no_change() {
    let input = "\"hello\" 'world'";
    assert_eq!(normalize_quotes(input), input);
}

// ---------------------------------------------------------------------------
// preserve_quote_style
// ---------------------------------------------------------------------------

#[test]
fn preserve_quote_style_no_normalization() {
    // old_text == actual_old_text → passthrough
    let result = preserve_quote_style("hello", "hello", "world");
    assert_eq!(result, "world");
}

#[test]
fn preserve_quote_style_double_curly() {
    let old = "say \"hello\"";
    let actual = "say \u{201C}hello\u{201D}";
    let new = "say \"goodbye\"";
    let result = preserve_quote_style(old, actual, new);
    assert_eq!(result, "say \u{201C}goodbye\u{201D}");
}

#[test]
fn preserve_quote_style_single_curly() {
    let old = "it's a 'test'";
    let actual = "it\u{2019}s a \u{2018}test\u{2019}";
    let new = "it's a 'demo'";
    let result = preserve_quote_style(old, actual, new);
    // apostrophe in "it's" → right single curly; 'demo' → curly pair
    assert_eq!(result, "it\u{2019}s a \u{2018}demo\u{2019}");
}

#[test]
fn preserve_quote_style_mixed() {
    let old = "\"hello\" and 'world'";
    let actual = "\u{201C}hello\u{201D} and \u{2018}world\u{2019}";
    let new = "\"goodbye\" and 'earth'";
    let result = preserve_quote_style(old, actual, new);
    assert_eq!(result, "\u{201C}goodbye\u{201D} and \u{2018}earth\u{2019}");
}

#[test]
fn preserve_quote_style_no_curly_in_actual() {
    // actual_old_text differs from old_text but has no curly quotes → passthrough
    let result = preserve_quote_style("abc", "def", "ghi");
    assert_eq!(result, "ghi");
}
