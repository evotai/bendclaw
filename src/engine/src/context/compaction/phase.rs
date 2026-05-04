//! Shared types for compaction phases.

use super::policy::CompactionPolicy;
use super::types::CompactionAction;
use crate::types::AgentMessage;

/// Shared read-only context passed to every phase.
pub struct PhaseContext {
    pub budget: usize,
    /// Collapse trigger: when context exceeds this, collapse runs.
    pub compact_trigger: usize,
    /// Compaction target: collapse and evict both aim to reduce context to this.
    pub compact_target: usize,
    pub keep_recent: usize,
    pub keep_first: usize,
    pub tool_output_max_lines: usize,
    pub policy: CompactionPolicy,
}

/// Output of a single phase.
pub struct PhaseResult {
    pub messages: Vec<AgentMessage>,
    pub actions: Vec<CompactionAction>,
}
