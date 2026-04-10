/// Mask a secret value: show first 2 and last 2 characters, replace the middle with `*`.
///
/// Values with 5 or fewer characters are fully masked.
pub fn mask_value(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 5 {
        return "*".repeat(chars.len());
    }
    let head: String = chars[..2].iter().collect();
    let tail: String = chars[chars.len() - 2..].iter().collect();
    let mid = "*".repeat(chars.len() - 4);
    format!("{head}{mid}{tail}")
}

/// Replace all known secret values in `text` with their masked form.
///
/// Secrets are sorted by length descending before replacement so that a longer
/// secret is masked before any shorter substring it might contain.
pub fn mask_secrets(text: &str, secrets: &[String]) -> String {
    if secrets.is_empty() {
        return text.to_string();
    }
    let mut sorted: Vec<&String> = secrets.iter().filter(|s| !s.is_empty()).collect();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.len()));
    sorted.dedup();
    let mut result = text.to_string();
    for secret in sorted {
        result = result.replace(secret.as_str(), &mask_value(secret));
    }
    result
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut value: String = s.chars().take(max).collect();
        value.push_str("...");
        value
    }
}

/// Truncate a string keeping both head and tail: `"head ... tail"`.
///
/// The separator ` ... ` costs 5 characters. If `max` is too small to fit
/// even a minimal head + separator + tail (< 12), falls back to plain `truncate`.
pub fn truncate_head_tail(s: &str, max: usize) -> String {
    const SEP: &str = " ... ";
    let sep_len = SEP.len();

    let char_count = s.chars().count();
    if char_count <= max || max < sep_len + 6 {
        return truncate(s, max);
    }

    let budget = max - sep_len;
    let head_len = budget / 2;
    let tail_len = budget - head_len;

    let head: String = s.chars().take(head_len).collect();
    let tail: String = s.chars().skip(char_count - tail_len).collect();

    format!("{}{SEP}{}", head.trim_end(), tail.trim_start())
}

pub fn summarize_inline(value: &str, max_chars: usize) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&collapsed, max_chars)
}

pub const SUMMARY_KEYS: &[&str] = &[
    "file_path",
    "path",
    "command",
    "pattern",
    "patterns",
    "query",
    "url",
    "name",
    "directory",
    "glob",
    "regex",
];

pub fn format_tool_input(input: &serde_json::Value) -> String {
    if let Some(obj) = input.as_object() {
        for &key in SUMMARY_KEYS {
            if let Some(val) = obj.get(key) {
                if let Some(s) = val.as_str() {
                    return summarize_inline(s, 100);
                }
                if let Some(arr) = val.as_array() {
                    let parts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                    if !parts.is_empty() {
                        return summarize_inline(&parts.join(", "), 100);
                    }
                }
            }
        }
    }
    summarize_inline(&input.to_string(), 100)
}

/// Format all tool input fields as separate lines for display.
pub fn format_tool_input_lines(input: &serde_json::Value) -> Vec<String> {
    if let Some(obj) = input.as_object() {
        if obj.is_empty() {
            return vec![];
        }
        return obj
            .iter()
            .map(|(k, v)| {
                let val = match v {
                    serde_json::Value::String(s) => summarize_inline(s, 120),
                    serde_json::Value::Array(arr) => {
                        let parts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                        summarize_inline(&parts.join(", "), 120)
                    }
                    other => summarize_inline(&other.to_string(), 120),
                };
                format!("{k}: {val}")
            })
            .collect();
    }
    vec![summarize_inline(&input.to_string(), 120)]
}
