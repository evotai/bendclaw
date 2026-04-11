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
