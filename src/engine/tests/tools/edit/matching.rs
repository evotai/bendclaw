use bendengine::tools::edit::matching::*;

#[test]
fn exact_unique() {
    let content = "fn main() {\n    println!(\"hello\");\n}\n";
    let old = "    println!(\"hello\");";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::Exact);
    assert_eq!(m.actual_old_text, old);
}

#[test]
fn exact_not_unique() {
    let content = "aaa\nbbb\naaa\n";
    let err = resolve_unique_match(content, "aaa").unwrap_err();
    assert_eq!(err, MatchError::NotUnique { count: 2 });
}

#[test]
fn empty_old_text() {
    let err = resolve_unique_match("content", "").unwrap_err();
    assert_eq!(err, MatchError::EmptyOldText);
}

#[test]
fn quote_normalized_match() {
    let content = "let s = \u{201C}hello\u{201D};\n";
    let old = "let s = \"hello\";";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::QuoteNormalized);
    assert_eq!(m.actual_old_text, "let s = \u{201C}hello\u{201D};");
}

#[test]
fn quote_normalized_reverse() {
    let content = "let s = \"hello\";\n";
    let old = "let s = \u{201C}hello\u{201D};";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::QuoteNormalized);
    assert_eq!(m.actual_old_text, "let s = \"hello\";");
}

#[test]
fn whitespace_insensitive_match() {
    let content = "fn foo() {   \n    bar();  \n}\n";
    let old = "fn foo() {\n    bar();\n}";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "fn foo() {   \n    bar();  \n}");
}

#[test]
fn whitespace_insensitive_old_has_trailing() {
    let content = "fn foo() {\n    bar();\n}\n";
    let old = "fn foo() {  \n    bar();  \n}";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "fn foo() {\n    bar();\n}");
}

#[test]
fn not_found() {
    let content = "fn main() {}\n";
    let err = resolve_unique_match(content, "nonexistent").unwrap_err();
    assert_eq!(err, MatchError::NotFound);
}

#[test]
fn find_similar_returns_context() {
    let content = "line1\nline2\nline3\nline4\n";
    let result = find_similar_text(content, "line2");
    assert!(result.is_some());
    assert!(result.unwrap().contains("line2"));
}

#[test]
fn find_similar_empty_target() {
    assert!(find_similar_text("content", "").is_none());
}

#[test]
fn whitespace_no_trailing_newline_at_eof() {
    let content = "aaa\nbbb";
    let old = "bbb";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::Exact);
    assert_eq!(m.actual_old_text, "bbb");
}

#[test]
fn whitespace_no_trailing_newline_at_eof_with_trailing_ws() {
    let content = "aaa\nbbb   ";
    let old = "bbb";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::Exact);
    assert_eq!(m.actual_old_text, "bbb");
}

#[test]
fn whitespace_no_trailing_newline_ws_only_via_fallback() {
    let content = "aaa\nbbb   ";
    let old = "aaa \nbbb";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "aaa\nbbb   ");
}

#[test]
fn whitespace_match_spans_to_eof_no_newline() {
    let content = "header\nfoo()   \nbar()  ";
    let old = "foo()\nbar()";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "foo()   \nbar()  ");
}

#[test]
fn whitespace_single_line_file() {
    let content = "only_line   ";
    let old = "only_line";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::Exact);
}

#[test]
fn whitespace_single_line_file_via_fallback() {
    let content = "only_line   ";
    let old = "only_line\t";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "only_line   ");
}

#[test]
fn whitespace_old_ends_with_newline() {
    let content = "aaa\nbbb  \nccc\n";
    let old = "bbb\n";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "bbb  \n");
}

#[test]
fn whitespace_match_at_start() {
    let content = "first   \nsecond\nthird\n";
    let old = "first\nsecond";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "first   \nsecond");
}
