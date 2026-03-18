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
