pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut value: String = s.chars().take(max).collect();
        value.push_str("...");
        value
    }
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
