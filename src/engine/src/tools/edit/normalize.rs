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

/// Preserve the curly-quote style from the file when the match was
/// quote-normalized.
///
/// When `actual_old_text` (from the file) contains curly quotes but
/// `old_text` (from the agent) used straight quotes, we apply the same
/// curly style to `new_text` so the replacement doesn't break the file's
/// typography.
///
/// If no quote normalization happened (`old_text == actual_old_text`),
/// returns `new_text` unchanged.
pub fn preserve_quote_style(old_text: &str, actual_old_text: &str, new_text: &str) -> String {
    if old_text == actual_old_text {
        return new_text.to_string();
    }

    let has_double = actual_old_text.contains('\u{201C}') || actual_old_text.contains('\u{201D}');
    let has_single = actual_old_text.contains('\u{2018}') || actual_old_text.contains('\u{2019}');

    if !has_double && !has_single {
        return new_text.to_string();
    }

    let mut result = new_text.to_string();
    if has_double {
        result = apply_curly_double_quotes(&result);
    }
    if has_single {
        result = apply_curly_single_quotes(&result);
    }
    result
}

/// Returns `true` if the character before `index` is whitespace, start of
/// string, or an opening bracket — i.e. the quote at `index` is an opening
/// quote.
fn is_opening_context(chars: &[char], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    matches!(
        chars[index - 1],
        ' ' | '\t' | '\n' | '\r' | '(' | '[' | '{' | '\u{2014}' | '\u{2013}'
    )
}

fn apply_curly_double_quotes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '"' {
            if is_opening_context(&chars, i) {
                out.push('\u{201C}'); // "
            } else {
                out.push('\u{201D}'); // "
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn apply_curly_single_quotes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' {
            // Apostrophe in a contraction (letter'letter) → right single curly
            let prev_is_letter = i > 0 && chars[i - 1].is_alphabetic();
            let next_is_letter = i + 1 < chars.len() && chars[i + 1].is_alphabetic();
            if prev_is_letter && next_is_letter {
                out.push('\u{2019}'); // '
            } else if is_opening_context(&chars, i) {
                out.push('\u{2018}'); // '
            } else {
                out.push('\u{2019}'); // '
            }
        } else {
            out.push(c);
        }
    }
    out
}
