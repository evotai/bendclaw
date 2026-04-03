use bendclaw::sessions::core::session_memory::MemoryFact;
use bendclaw::sessions::core::session_memory::SessionMemory;

#[test]
fn empty_memory() {
    let mem = SessionMemory::default();
    assert!(mem.is_empty());
}

#[test]
fn memory_with_facts() {
    let mem = SessionMemory {
        memory_text: String::new(),
        facts: vec![MemoryFact {
            content: "user prefers dark mode".into(),
            source_run_id: "r01".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        }],
    };
    assert!(!mem.is_empty());
}

#[test]
fn memory_serde_roundtrip() {
    let mem = SessionMemory {
        memory_text: "accumulated".into(),
        facts: vec![MemoryFact {
            content: "fact".into(),
            source_run_id: "r01".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        }],
    };
    let json = serde_json::to_string(&mem).unwrap();
    let back: SessionMemory = serde_json::from_str(&json).unwrap();
    assert_eq!(back.memory_text, "accumulated");
    assert_eq!(back.facts.len(), 1);
}
