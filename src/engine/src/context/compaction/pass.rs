//! Shared types for compaction passes.

use super::compact::CompactionAction;
use super::policy::CompactionPolicy;
use crate::types::AgentMessage;

/// Shared read-only context passed to every pass.
pub struct CompactContext {
    pub budget: usize,
    /// L1 collapse trigger: when context exceeds this, L1 runs.
    pub compact_trigger: usize,
    /// Compaction target: L1 and L2 both aim to reduce context to this.
    pub compact_target: usize,
    pub keep_recent: usize,
    pub keep_first: usize,
    pub tool_output_max_lines: usize,
    pub policy: CompactionPolicy,
}

/// Output of a single pass.
pub struct PassResult {
    pub messages: Vec<AgentMessage>,
    pub actions: Vec<CompactionAction>,
}
