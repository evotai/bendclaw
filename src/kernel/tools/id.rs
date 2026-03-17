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
    ListDir,
    Shell,
    Databend,
    ChannelSend,
    WebSearch,
    WebFetch,
    TaskCreate,
    TaskList,
    TaskGet,
    TaskUpdate,
    TaskDelete,
    TaskToggle,
    TaskHistory,
    LearningWrite,
    KnowledgeSearch,
    LearningSearch,
    ClusterNodes,
    ClusterDispatch,
    ClusterCollect,
    ClaudeCode,
    CodexExec,
    CodeReview,
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
            Self::ListDir => "list_dir",
            Self::Shell => "shell",
            Self::Databend => "databend",
            Self::ChannelSend => "channel_send",
            Self::WebSearch => "web_search",
            Self::WebFetch => "web_fetch",
            Self::TaskCreate => "task_create",
            Self::TaskList => "task_list",
            Self::TaskGet => "task_get",
            Self::TaskUpdate => "task_update",
            Self::TaskDelete => "task_delete",
            Self::TaskToggle => "task_toggle",
            Self::TaskHistory => "task_history",
            Self::LearningWrite => "learning_write",
            Self::KnowledgeSearch => "knowledge_search",
            Self::LearningSearch => "learning_search",
            Self::ClusterNodes => "cluster_nodes",
            Self::ClusterDispatch => "cluster_dispatch",
            Self::ClusterCollect => "cluster_collect",
            Self::ClaudeCode => "claude_code",
            Self::CodexExec => "codex_exec",
            Self::CodeReview => "code_review",
        }
    }
}

pub const CHECKPOINT_MEMORY_TOOLS: [ToolId; 3] = [
    ToolId::MemoryWrite,
    ToolId::MemorySearch,
    ToolId::MemoryRead,
];

impl ToolId {
    /// Every `ToolId` variant, grouped by category.
    pub const ALL: &[ToolId] = &[
        // Memory
        ToolId::MemoryWrite,
        ToolId::MemorySearch,
        ToolId::MemoryRead,
        ToolId::MemoryDelete,
        ToolId::MemoryList,
        // Skills
        ToolId::SkillRead,
        ToolId::SkillCreate,
        ToolId::SkillRemove,
        // Files
        ToolId::FileRead,
        ToolId::FileWrite,
        ToolId::FileEdit,
        ToolId::ListDir,
        // Shell
        ToolId::Shell,
        // Integrations
        ToolId::Databend,
        ToolId::ChannelSend,
        // Web
        ToolId::WebSearch,
        ToolId::WebFetch,
        // Tasks
        ToolId::TaskCreate,
        ToolId::TaskList,
        ToolId::TaskGet,
        ToolId::TaskUpdate,
        ToolId::TaskDelete,
        ToolId::TaskToggle,
        ToolId::TaskHistory,
        // Recall
        ToolId::LearningWrite,
        ToolId::KnowledgeSearch,
        ToolId::LearningSearch,
        // Coding agents
        ToolId::ClaudeCode,
        ToolId::CodexExec,
        ToolId::CodeReview,
    ];

    /// Cluster tools, registered conditionally when cluster config is present.
    pub const CLUSTER: &[ToolId] = &[
        ToolId::ClusterNodes,
        ToolId::ClusterDispatch,
        ToolId::ClusterCollect,
    ];
}
