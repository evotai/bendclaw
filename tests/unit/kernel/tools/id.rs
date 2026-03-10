use bendclaw::kernel::tools::id::CHECKPOINT_MEMORY_TOOLS;
use bendclaw::kernel::tools::id::RESERVED_TOOL_IDS;
use bendclaw::kernel::tools::ToolId;

#[test]
fn tool_id_debug() {
    assert_eq!(format!("{:?}", ToolId::Shell), "Shell");
    assert_eq!(format!("{:?}", ToolId::Databend), "Databend");
    assert_eq!(format!("{:?}", ToolId::MemoryWrite), "MemoryWrite");
}

#[test]
fn tool_id_clone_and_copy() {
    let a = ToolId::FileRead;
    let b = a; // Copy
    let c = a; // also Copy (ToolId is Copy)
    assert_eq!(a, b);
    assert_eq!(a, c);
}

#[test]
fn checkpoint_memory_tools_contents() {
    assert!(CHECKPOINT_MEMORY_TOOLS.contains(&ToolId::MemoryWrite));
    assert!(CHECKPOINT_MEMORY_TOOLS.contains(&ToolId::MemorySearch));
    assert!(CHECKPOINT_MEMORY_TOOLS.contains(&ToolId::MemoryRead));
}

#[test]
fn reserved_tool_ids_contains_all_variants() {
    let all = [
        ToolId::MemoryWrite,
        ToolId::MemorySearch,
        ToolId::MemoryRead,
        ToolId::MemoryDelete,
        ToolId::MemoryList,
        ToolId::SkillRead,
        ToolId::SkillCreate,
        ToolId::SkillRemove,
        ToolId::FileRead,
        ToolId::FileWrite,
        ToolId::FileEdit,
        ToolId::Shell,
        ToolId::Databend,
    ];
    for id in all {
        assert!(
            RESERVED_TOOL_IDS.contains(&id),
            "{:?} missing from RESERVED_TOOL_IDS",
            id
        );
    }
}

#[test]
fn tool_id_as_str() {
    assert_eq!(ToolId::MemoryWrite.as_str(), "memory_write");
    assert_eq!(ToolId::MemorySearch.as_str(), "memory_search");
    assert_eq!(ToolId::MemoryRead.as_str(), "memory_read");
    assert_eq!(ToolId::MemoryDelete.as_str(), "memory_delete");
    assert_eq!(ToolId::MemoryList.as_str(), "memory_list");
    assert_eq!(ToolId::SkillRead.as_str(), "skill_read");
    assert_eq!(ToolId::SkillCreate.as_str(), "create_skill");
    assert_eq!(ToolId::SkillRemove.as_str(), "remove_skill");
    assert_eq!(ToolId::FileRead.as_str(), "file_read");
    assert_eq!(ToolId::FileWrite.as_str(), "file_write");
    assert_eq!(ToolId::FileEdit.as_str(), "file_edit");
    assert_eq!(ToolId::Shell.as_str(), "shell");
    assert_eq!(ToolId::Databend.as_str(), "databend");
}

#[test]
fn tool_id_eq_and_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(ToolId::Shell);
    set.insert(ToolId::Shell);
    assert_eq!(set.len(), 1);
    assert!(set.contains(&ToolId::Shell));
}

#[test]
fn checkpoint_memory_tools_count() {
    assert_eq!(
        bendclaw::kernel::tools::id::CHECKPOINT_MEMORY_TOOLS.len(),
        3
    );
}

#[test]
fn reserved_tool_ids_count() {
    assert_eq!(bendclaw::kernel::tools::id::RESERVED_TOOL_IDS.len(), 14);
}
