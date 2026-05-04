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
pub mod image_resize;
pub mod tokens;
pub mod tracking;

pub use compaction::compact_messages;
pub use compaction::sanitize_tool_pairs;
pub use compaction::truncate_text_head_tail;
pub use compaction::CompactionAction;
pub use compaction::CompactionMethod;
pub use compaction::CompactionResult;
pub use compaction::CompactionStats;
pub use compaction::CompactionStrategy;
pub use compaction::DefaultCompaction;
pub use compaction::ToolTokenDetail;
pub use image_resize::resize_image;
pub use tokens::compute_call_stats;
pub use tokens::compute_call_stats_from_agent_messages;
pub use tokens::content_tokens;
pub use tokens::estimate_tokens;
pub use tokens::message_tokens;
pub use tokens::tool_definition_tokens;
pub use tokens::total_tokens;
pub use tracking::CompactionBudgetState;
pub use tracking::ContextBudgetSnapshot;
pub use tracking::ContextConfig;
pub use tracking::ContextTracker;
pub use tracking::ExecutionLimits;
pub use tracking::ExecutionTracker;
