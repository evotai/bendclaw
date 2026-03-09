use bendclaw::kernel::agent_store::memory_store::build_search_extra_where;
use bendclaw::kernel::agent_store::memory_store::parse_scope;
use bendclaw::kernel::agent_store::memory_store::visibility_where;
use bendclaw::kernel::agent_store::memory_store::MemoryEntry;
use bendclaw::kernel::agent_store::memory_store::MemoryResult;
use bendclaw::kernel::agent_store::memory_store::MemoryScope;
use bendclaw::kernel::agent_store::memory_store::SearchOpts;

// ── MemoryEntry ──

#[test]
fn memory_entry_serde_roundtrip() {
    let entry = MemoryEntry {
        id: "m1".into(),
        user_id: "u1".into(),
        scope: MemoryScope::User,
        session_id: None,
        key: "pref".into(),
        content: "dark mode".into(),
        created_at: "2026-01-01".into(),
        updated_at: "2026-01-02".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: MemoryEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "m1");
    assert_eq!(back.scope, MemoryScope::User);
    assert!(back.session_id.is_none());
}

#[test]
fn memory_entry_with_session() {
    let entry = MemoryEntry {
        id: "m2".into(),
        user_id: "u1".into(),
        scope: MemoryScope::Session,
        session_id: Some("s1".into()),
        key: "temp".into(),
        content: "temporary data".into(),
        created_at: "".into(),
        updated_at: "".into(),
    };
    assert_eq!(entry.scope, MemoryScope::Session);
    assert_eq!(entry.session_id.as_deref(), Some("s1"));
}

#[test]
fn memory_result_serde_roundtrip() {
    let result = MemoryResult {
        id: "m1".into(),
        key: "pref".into(),
        content: "dark mode".into(),
        scope: MemoryScope::Shared,
        session_id: None,
        score: 0.95,
        updated_at: "2026-01-01".into(),
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: MemoryResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.key, "pref");
    assert_eq!(back.scope, MemoryScope::Shared);
    assert!((back.score - 0.95).abs() < f32::EPSILON);
}

#[test]
fn search_opts_defaults() {
    let opts = SearchOpts::default();
    assert_eq!(opts.max_results, 10);
    assert!(opts.include_shared);
    assert!(opts.session_id.is_none());
    assert_eq!(opts.min_score, 0.0);
}

#[test]
fn memory_scope_display_all() {
    assert_eq!(MemoryScope::User.to_string(), "user");
    assert_eq!(MemoryScope::Shared.to_string(), "shared");
    assert_eq!(MemoryScope::Session.to_string(), "session");
}

#[test]
fn memory_scope_serde_roundtrip() {
    for scope in [MemoryScope::User, MemoryScope::Shared, MemoryScope::Session] {
        let json = serde_json::to_string(&scope).unwrap();
        let back: MemoryScope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, scope);
    }
}

// ── parse_scope ──

#[test]
fn parse_scope_maps_known_values_and_defaults_to_user() {
    assert_eq!(parse_scope("shared"), MemoryScope::Shared);
    assert_eq!(parse_scope("session"), MemoryScope::Session);
    assert_eq!(parse_scope("user"), MemoryScope::User);
    assert_eq!(parse_scope("unknown"), MemoryScope::User);
}

#[test]
fn parse_scope_empty_string_defaults_to_user() {
    assert_eq!(parse_scope(""), MemoryScope::User);
}

#[test]
fn parse_scope_case_sensitive() {
    assert_eq!(parse_scope("Shared"), MemoryScope::User);
    assert_eq!(parse_scope("SESSION"), MemoryScope::User);
    assert_eq!(parse_scope("User"), MemoryScope::User);
}

// ── visibility_where ──

#[test]
fn visibility_where_includes_shared_when_enabled() {
    let sql = visibility_where("u1", true);
    assert!(sql.contains("user_id = 'u1'"));
    assert!(sql.contains("scope = 'shared'"));
    assert!(sql.contains(" OR "));
}

#[test]
fn visibility_where_only_user_when_shared_disabled() {
    let sql = visibility_where("u1", false);
    assert_eq!(sql, "user_id = 'u1'");
}

#[test]
fn visibility_where_escapes_special_chars() {
    let sql = visibility_where("user's\"name", true);
    assert!(sql.contains("user''s"));
}

// ── build_search_extra_where ──

#[test]
fn build_search_extra_where_combines_all_filters() {
    let opts = SearchOpts {
        max_results: 5,
        include_shared: false,
        session_id: Some("s1".to_string()),
        min_score: 0.7,
    };

    let sql = build_search_extra_where("u1", &opts);
    assert!(sql.contains("user_id = 'u1'"));
    assert!(sql.contains("session_id = 's1'"));
    assert!(sql.contains("SCORE() >= 0.7"));
    assert!(sql.contains(" AND "));
}

#[test]
fn build_search_extra_where_escapes_sql_literals() {
    let opts = SearchOpts::default();
    let sql = build_search_extra_where("o'reilly", &opts);
    assert!(sql.contains("o''reilly"));
}

#[test]
fn build_search_extra_where_defaults_only_visibility() {
    let opts = SearchOpts::default();
    let sql = build_search_extra_where("u1", &opts);
    assert!(sql.contains("user_id = 'u1'"));
    assert!(sql.contains("scope = 'shared'"));
    assert!(!sql.contains("session_id"));
    assert!(!sql.contains("SCORE()"));
}

#[test]
fn build_search_extra_where_session_only() {
    let opts = SearchOpts {
        max_results: 10,
        include_shared: true,
        session_id: Some("sess-42".to_string()),
        min_score: 0.0,
    };
    let sql = build_search_extra_where("u1", &opts);
    assert!(sql.contains("session_id = 'sess-42'"));
    assert!(!sql.contains("SCORE()"));
}

#[test]
fn build_search_extra_where_min_score_only() {
    let opts = SearchOpts {
        max_results: 10,
        include_shared: false,
        session_id: None,
        min_score: 0.5,
    };
    let sql = build_search_extra_where("u1", &opts);
    assert!(sql.contains("SCORE() >= 0.5"));
    assert!(!sql.contains("session_id"));
    assert!(!sql.contains("scope = 'shared'"));
}

#[test]
fn build_search_extra_where_session_id_escaped() {
    let opts = SearchOpts {
        max_results: 5,
        include_shared: false,
        session_id: Some("it's-a-session".to_string()),
        min_score: 0.0,
    };
    let sql = build_search_extra_where("u1", &opts);
    assert!(sql.contains("it''s-a-session"));
}
