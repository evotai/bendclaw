use std::collections::HashSet;

pub const PRESET_FILE: &[&str] = &["read", "write", "edit", "list_dir"];
pub const PRESET_SEARCH: &[&str] = &["grep", "glob"];
pub const PRESET_SHELL: &[&str] = &["bash"];
pub const PRESET_WEB: &[&str] = &["web_search", "web_fetch"];
pub const PRESET_CODING: &[&str] = &["read", "write", "edit", "list_dir", "grep", "glob", "bash"];

/// Parse a comma-separated tool selection string into a filter set.
/// "all" → None (no filter). Otherwise expands presets and individual names.
pub fn parse_tool_selection(s: &str) -> Option<HashSet<String>> {
    if s == "all" {
        return None;
    }
    let mut set = HashSet::new();
    for token in s.split(',').map(str::trim) {
        match token {
            "file" => set.extend(PRESET_FILE.iter().map(|s| s.to_string())),
            "search" => set.extend(PRESET_SEARCH.iter().map(|s| s.to_string())),
            "shell" => set.extend(PRESET_SHELL.iter().map(|s| s.to_string())),
            "web" => set.extend(PRESET_WEB.iter().map(|s| s.to_string())),
            "coding" => set.extend(PRESET_CODING.iter().map(|s| s.to_string())),
            other => {
                set.insert(other.to_string());
            }
        }
    }
    Some(set)
}
