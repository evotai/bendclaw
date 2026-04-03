use bendclaw::memory::format::format_for_prompt;
use bendclaw::memory::MemoryEntry;
use bendclaw::memory::MemoryScope;

fn entry(key: &str, content: &str) -> MemoryEntry {
    MemoryEntry {
        id: key.into(),
        user_id: "u1".into(),
        agent_id: "a1".into(),
        scope: MemoryScope::Agent,
        key: key.into(),
        content: content.into(),
        access_count: 0,
        last_accessed_at: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

#[test]
fn empty_entries_returns_none() {
    assert!(format_for_prompt(&[], 2000).is_none());
}

#[test]
fn budget_zero_returns_none() {
    let entries = vec![entry("k", "v")];
    assert!(format_for_prompt(&entries, 0).is_none());
}

#[test]
fn budget_too_small_returns_none() {
    let entries = vec![entry("k", "v")];
    assert!(format_for_prompt(&entries, 10).is_none());
}

#[test]
fn single_entry_fits() {
    let result = format_for_prompt(&[entry("tz", "UTC+8")], 2000).unwrap();
    assert!(result.contains("## Memory"));
    assert!(result.contains("- tz: UTC+8"));
}

#[test]
fn multiple_entries_fit() {
    let entries = vec![entry("tz", "UTC+8"), entry("lang", "Rust")];
    let result = format_for_prompt(&entries, 2000).unwrap();
    assert!(result.contains("- tz: UTC+8"));
    assert!(result.contains("- lang: Rust"));
}

#[test]
fn budget_truncates_second_entry() {
    let entries = vec![
        entry("k1", "short"),
        entry("k2", "this is a much longer fact that exceeds the budget"),
    ];
    let result = format_for_prompt(&entries, 40).unwrap();
    assert!(result.contains("k1: short"));
    assert!(!result.contains("k2:"));
}

#[test]
fn budget_below_min_returns_none() {
    let entries = vec![entry("k", "v")];
    // "## Memory\n" = 10, "- k: v\n" = 7, total = 17 — but min budget is 20
    assert!(format_for_prompt(&entries, 17).is_none());
    // At budget=20, the entry fits
    let result = format_for_prompt(&entries, 20).unwrap();
    assert!(result.contains("- k: v"));
}

#[test]
fn first_entry_too_large_returns_none() {
    let entries = vec![entry("k", "a very long value that will not fit")];
    assert!(format_for_prompt(&entries, 25).is_none());
}
