//! Context window management — smart truncation and token counting.
//!
//! The #1 engineering challenge for agents. This module provides:
//! - Token estimation (fast, no external deps)
//! - Tiered compaction (tool output truncation → turn summarization → full summary)
//! - Execution limits (max turns, tokens, duration)
//!
//! Designed based on Claude Code's approach: clear old tool outputs first,
//! then summarize conversation if needed.

pub mod compaction;
pub mod tokens;
pub mod tracking;

pub use compaction::compact_messages;
pub use compaction::level1_truncate_tool_outputs;
pub use compaction::sanitize_tool_pairs;
pub use compaction::truncate_text_head_tail;
pub use compaction::CompactionResult;
pub use compaction::CompactionStats;
pub use compaction::CompactionStrategy;
pub use compaction::DefaultCompaction;
pub use compaction::ToolTokenDetail;
pub use tokens::content_tokens;
pub use tokens::estimate_tokens;
pub use tokens::message_tokens;
pub use tokens::total_tokens;
pub use tracking::ContextConfig;
pub use tracking::ContextTracker;
pub use tracking::ExecutionLimits;
pub use tracking::ExecutionTracker;
