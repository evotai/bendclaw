//! Text normalization primitives for edit matching.
//!
//! All functions are pure — no IO, no side effects.

/// Line ending style detected in file content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

/// Detect the dominant line ending in `content` by counting occurrences.
pub fn detect_line_ending(content: &str) -> LineEnding {
    let crlf = content.matches("\r\n").count();
    // Total LF minus those that are part of CRLF = standalone LF count
    let lf = content.matches('\n').count().saturating_sub(crlf);
    if crlf > lf {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    }
}

/// Normalize all line endings to LF.
pub fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Restore line endings from LF to the given style.
pub fn restore_line_endings(text: &str, ending: LineEnding) -> String {
    match ending {
        LineEnding::Lf => text.to_string(),
        LineEnding::CrLf => text.replace('\n', "\r\n"),
    }
}

/// Strip UTF-8 BOM if present.
/// Returns `(bom_str, content_without_bom)` where `bom_str` is `"\u{FEFF}"` or `""`.
pub fn strip_utf8_bom(content: &str) -> (&str, &str) {
    if let Some(stripped) = content.strip_prefix('\u{FEFF}') {
        ("\u{FEFF}", stripped)
    } else {
        ("", content)
    }
}

/// Normalize curly/smart quotes to ASCII straight quotes.
///
/// IMPORTANT: This is a length-preserving, 1:1 Unicode character replacement.
/// Each curly quote maps to exactly one straight quote character.
/// This invariant is relied upon by `matching.rs` to map char indices
/// between normalized and original content. Do not add any transformation
/// that changes character count.
pub fn normalize_quotes(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'', // ' ' → '
            '\u{201C}' | '\u{201D}' => '"',  // " " → "
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Length-preserving: same char count
        assert_eq!(input.chars().count(), result.chars().count());
    }

    #[test]
    fn normalize_quotes_no_change() {
        let input = "\"hello\" 'world'";
        assert_eq!(normalize_quotes(input), input);
    }
}
