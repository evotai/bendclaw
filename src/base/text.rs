pub fn truncate_chars_with_ellipsis(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let keep = max_chars - 3;
    let mut end = text.len();
    for (seen, (idx, _)) in text.char_indices().enumerate() {
        if seen == keep {
            end = idx;
            break;
        }
    }

    format!("{}...", &text[..end])
}

pub fn truncate_bytes_on_char_boundary(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }

    let end = text.floor_char_boundary(max_bytes);
    text[..end].to_string()
}

/// Truncate keeping head (70%) + tail (30%) on char boundaries.
/// Tail often contains error messages, so this is more useful than head-only truncation.
pub fn truncate_head_tail(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let marker = "\n... [truncated] ...\n";
    let usable = max_bytes.saturating_sub(marker.len());
    if usable == 0 {
        return marker[..max_bytes].to_string();
    }
    let head_budget = usable * 7 / 10;
    let tail_budget = usable - head_budget;
    let head_end = text.floor_char_boundary(head_budget);
    let tail_start = text.ceil_char_boundary(text.len().saturating_sub(tail_budget));
    format!("{}{marker}{}", &text[..head_end], &text[tail_start..])
}

/// Truncate text to `max_bytes` on a char boundary, appending a notice if truncated.
pub fn truncate_with_notice(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let truncated = truncate_bytes_on_char_boundary(text, max_bytes);
    format!(
        "{truncated}\n\n[truncated: showing {}/{} bytes]",
        truncated.len(),
        text.len()
    )
}
