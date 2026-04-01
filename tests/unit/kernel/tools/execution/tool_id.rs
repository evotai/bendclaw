use bendclaw::kernel::tools::ToolId;

#[test]
fn tool_id_debug() {
    assert_eq!(format!("{:?}", ToolId::Bash), "Bash");
    assert_eq!(format!("{:?}", ToolId::Databend), "Databend");
    assert_eq!(format!("{:?}", ToolId::Read), "Read");
}

#[test]
fn tool_id_clone_and_copy() {
    let a = ToolId::Read;
    let b = a; // Copy
    let c = a; // also Copy (ToolId is Copy)
    assert_eq!(a, b);
    assert_eq!(a, c);
}

#[test]
fn all_variants_have_unique_str() {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for id in ToolId::ALL {
        assert!(
            seen.insert(id.as_str()),
            "duplicate as_str value: {}",
            id.as_str()
        );
    }
}

#[test]
fn tool_id_as_str() {
    assert_eq!(ToolId::SkillRead.as_str(), "skill_read");
    assert_eq!(ToolId::SkillCreate.as_str(), "create_skill");
    assert_eq!(ToolId::SkillRemove.as_str(), "remove_skill");
    assert_eq!(ToolId::Read.as_str(), "read");
    assert_eq!(ToolId::Write.as_str(), "write");
    assert_eq!(ToolId::Edit.as_str(), "edit");
    assert_eq!(ToolId::Bash.as_str(), "bash");
    assert_eq!(ToolId::Databend.as_str(), "databend");
}

#[test]
fn tool_id_eq_and_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(ToolId::Bash);
    set.insert(ToolId::Bash);
    assert_eq!(set.len(), 1);
    assert!(set.contains(&ToolId::Bash));
}
