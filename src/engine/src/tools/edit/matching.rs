//! Matching logic for edit tool — find the unique occurrence of old_text in file content.
//!
//! Uses a tiered fallback strategy:
//! 1. Exact match
//! 2. Quote-normalized match (curly quotes → straight quotes)
//! 3. Trailing-whitespace-insensitive line match
//!
//! All functions are pure — no IO, no side effects.

use super::normalize;

/// How the match was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Exact,
    QuoteNormalized,
    WhitespaceInsensitive,
}

/// A successfully resolved match — contains the actual text from the original
/// file content that can be used directly with `replacen`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMatch {
    pub actual_old_text: String,
    pub kind: MatchKind,
}

/// Matching errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchError {
    EmptyOldText,
    NotFound,
    NotUnique { count: usize },
}

/// Resolve a unique match of `old_text_lf` within `content_lf`.
///
/// Both inputs must already be LF-normalized.
/// Returns the actual text slice from `content_lf` that should be replaced.
pub fn resolve_unique_match(
    content_lf: &str,
    old_text_lf: &str,
) -> Result<ResolvedMatch, MatchError> {
    if old_text_lf.is_empty() {
        return Err(MatchError::EmptyOldText);
    }

    // Level 1: Exact match
    let count = content_lf.matches(old_text_lf).count();
    if count == 1 {
        return Ok(ResolvedMatch {
            actual_old_text: old_text_lf.to_string(),
            kind: MatchKind::Exact,
        });
    }
    if count > 1 {
        return Err(MatchError::NotUnique { count });
    }

    // Level 2: Quote-normalized match
    if let Some(result) = try_quote_normalized(content_lf, old_text_lf) {
        return result;
    }

    // Level 3: Trailing-whitespace-insensitive line match
    if let Some(result) = try_whitespace_insensitive(content_lf, old_text_lf) {
        return result;
    }

    Err(MatchError::NotFound)
}

/// Try matching after normalizing curly quotes to straight quotes.
///
/// Because `normalize_quotes` is a 1:1 char mapping, char indices in the
/// normalized string correspond exactly to char indices in the original.
fn try_quote_normalized(
    content_lf: &str,
    old_text_lf: &str,
) -> Option<Result<ResolvedMatch, MatchError>> {
    let norm_content = normalize::normalize_quotes(content_lf);
    let norm_old = normalize::normalize_quotes(old_text_lf);

    // If normalization didn't change anything, skip (already tried exact)
    if norm_content == content_lf && norm_old == old_text_lf {
        return None;
    }

    let norm_old_chars: Vec<char> = norm_old.chars().collect();
    let norm_content_chars: Vec<char> = norm_content.chars().collect();
    let search_len = norm_old_chars.len();

    if search_len == 0 || norm_content_chars.len() < search_len {
        return None;
    }

    let mut matches: Vec<usize> = Vec::new();
    for i in 0..=norm_content_chars.len() - search_len {
        if norm_content_chars[i..i + search_len] == norm_old_chars[..] {
            matches.push(i);
        }
    }

    match matches.len() {
        0 => None,
        1 => {
            // Map char index back to original content
            let start_char = matches[0];
            let actual: String = content_lf
                .chars()
                .skip(start_char)
                .take(search_len)
                .collect();
            Some(Ok(ResolvedMatch {
                actual_old_text: actual,
                kind: MatchKind::QuoteNormalized,
            }))
        }
        count => Some(Err(MatchError::NotUnique { count })),
    }
}

/// A line span: byte range `[start, end)` within the source string.
/// `end` points past the `\n` if one exists, or to the string end for the last line.
struct LineSpan {
    start: usize,
    /// Byte offset one past the end of this line (including its `\n`, if any).
    end: usize,
    /// Byte offset of the end of the line content (excluding `\n`).
    content_end: usize,
}

/// Build a line span table for `text`. Each entry records the byte range of one line.
fn build_line_spans(text: &str) -> Vec<LineSpan> {
    let mut spans = Vec::new();
    let mut pos = 0;
    for line in text.split('\n') {
        let content_end = pos + line.len();
        // end includes the '\n' if present, otherwise stops at string end
        let end = if content_end < text.len() {
            content_end + 1
        } else {
            content_end
        };
        spans.push(LineSpan {
            start: pos,
            end,
            content_end,
        });
        pos = end;
    }
    spans
}

/// Try matching lines with trailing whitespace stripped on both sides.
fn try_whitespace_insensitive(
    content_lf: &str,
    old_text_lf: &str,
) -> Option<Result<ResolvedMatch, MatchError>> {
    let old_lines: Vec<&str> = old_text_lf.lines().collect();
    let content_spans = build_line_spans(content_lf);

    if old_lines.is_empty() || content_spans.len() < old_lines.len() {
        return None;
    }

    let mut matches: Vec<usize> = Vec::new();
    for i in 0..=content_spans.len() - old_lines.len() {
        let all_match = old_lines.iter().enumerate().all(|(j, old_line)| {
            let span = &content_spans[i + j];
            content_lf[span.start..span.content_end].trim_end() == old_line.trim_end()
        });
        if all_match {
            matches.push(i);
        }
    }

    match matches.len() {
        0 => None,
        1 => {
            let start = matches[0];
            let last = start + old_lines.len() - 1;
            let byte_start = content_spans[start].start;
            // Use content_end of the last matched line (excludes trailing \n),
            // then include the \n only if old_text itself ended with one.
            let byte_end = if old_text_lf.ends_with('\n') {
                content_spans[last].end
            } else {
                content_spans[last].content_end
            };
            let actual = content_lf[byte_start..byte_end].to_string();
            Some(Ok(ResolvedMatch {
                actual_old_text: actual,
                kind: MatchKind::WhitespaceInsensitive,
            }))
        }
        count => Some(Err(MatchError::NotUnique { count })),
    }
}

/// Try to find similar text in the file content for error hints.
///
/// Looks for lines containing the first line of `target` and returns
/// a snippet of surrounding context.
pub fn find_similar_text(content: &str, target: &str) -> Option<String> {
    let target_trimmed = target.trim();
    if target_trimmed.is_empty() {
        return None;
    }

    let first_line = target_trimmed.lines().next()?;
    let first_line_trimmed = first_line.trim();

    if first_line_trimmed.is_empty() {
        return None;
    }

    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains(first_line_trimmed) {
            let start = i;
            let target_line_count = target_trimmed.lines().count();
            let end = (i + target_line_count + 1).min(lines.len());
            return Some(lines[start..end].join("\n"));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // File has curly quotes, LLM sends straight quotes
        let content = "let s = \u{201C}hello\u{201D};\n";
        let old = "let s = \"hello\";";
        let m = resolve_unique_match(content, old).unwrap();
        assert_eq!(m.kind, MatchKind::QuoteNormalized);
        // actual_old_text should be the original content with curly quotes
        assert_eq!(m.actual_old_text, "let s = \u{201C}hello\u{201D};");
    }

    #[test]
    fn quote_normalized_reverse() {
        // File has straight quotes, LLM sends curly quotes
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
        // actual_old_text preserves original trailing whitespace
        assert_eq!(m.actual_old_text, "fn foo() {   \n    bar();  \n}");
    }

    #[test]
    fn whitespace_insensitive_old_has_trailing() {
        // old_text has trailing whitespace, content doesn't
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

    // --- Edge cases for whitespace fallback ---

    #[test]
    fn whitespace_no_trailing_newline_at_eof() {
        // File has no trailing newline, match at end
        let content = "aaa\nbbb";
        let old = "bbb";
        let m = resolve_unique_match(content, old).unwrap();
        assert_eq!(m.kind, MatchKind::Exact);
        assert_eq!(m.actual_old_text, "bbb");
    }

    #[test]
    fn whitespace_no_trailing_newline_at_eof_with_trailing_ws() {
        // File: no trailing newline, last line has trailing whitespace
        // "bbb" is a substring of "bbb   " so exact match fires — that's correct
        let content = "aaa\nbbb   ";
        let old = "bbb";
        let m = resolve_unique_match(content, old).unwrap();
        assert_eq!(m.kind, MatchKind::Exact);
        assert_eq!(m.actual_old_text, "bbb");
    }

    #[test]
    fn whitespace_no_trailing_newline_ws_only_via_fallback() {
        // old_text lines differ only in trailing whitespace from content lines
        // "aaa \nbbb" is NOT a substring of "aaa\nbbb   " (note space after aaa)
        let content = "aaa\nbbb   ";
        let old = "aaa \nbbb";
        let m = resolve_unique_match(content, old).unwrap();
        assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
        assert_eq!(m.actual_old_text, "aaa\nbbb   ");
    }

    #[test]
    fn whitespace_match_spans_to_eof_no_newline() {
        // Multi-line match at end of file without trailing newline
        let content = "header\nfoo()   \nbar()  ";
        let old = "foo()\nbar()";
        let m = resolve_unique_match(content, old).unwrap();
        assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
        assert_eq!(m.actual_old_text, "foo()   \nbar()  ");
    }

    #[test]
    fn whitespace_single_line_file() {
        // "only_line" is substring of "only_line   " so exact match fires
        let content = "only_line   ";
        let old = "only_line";
        let m = resolve_unique_match(content, old).unwrap();
        assert_eq!(m.kind, MatchKind::Exact);
    }

    #[test]
    fn whitespace_single_line_file_via_fallback() {
        // old_text has trailing ws that is NOT a substring of content
        // "only_line \t" won't appear in "only_line   " — forces whitespace fallback
        let content = "only_line   ";
        let old = "only_line\t";
        let m = resolve_unique_match(content, old).unwrap();
        assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
        assert_eq!(m.actual_old_text, "only_line   ");
    }

    #[test]
    fn whitespace_old_ends_with_newline() {
        // old_text ends with \n — actual should include the \n from content
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
}
