#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolId {
    MemoryWrite,
    MemorySearch,
    MemoryRead,
    MemoryDelete,
    MemoryList,
    SkillRead,
    SkillCreate,
    SkillRemove,
    FileRead,
    FileWrite,
    FileEdit,
    Shell,
    Databend,
    ChannelSend,
}

impl ToolId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MemoryWrite => "memory_write",
            Self::MemorySearch => "memory_search",
            Self::MemoryRead => "memory_read",
            Self::MemoryDelete => "memory_delete",
            Self::MemoryList => "memory_list",
            Self::SkillRead => "skill_read",
            Self::SkillCreate => "create_skill",
            Self::SkillRemove => "remove_skill",
            Self::FileRead => "file_read",
            Self::FileWrite => "file_write",
            Self::FileEdit => "file_edit",
            Self::Shell => "shell",
            Self::Databend => "databend",
            Self::ChannelSend => "channel_send",
        }
    }
}

pub const CHECKPOINT_MEMORY_TOOLS: [ToolId; 3] = [
    ToolId::MemoryWrite,
    ToolId::MemorySearch,
    ToolId::MemoryRead,
];

/// Reserved tool names that cannot be used as skill names.
pub const RESERVED_TOOL_IDS: [ToolId; 14] = [
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
    ToolId::ChannelSend,
];
