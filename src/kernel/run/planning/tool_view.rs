//! Progressive tool view — token-efficient on-demand tool schema expansion.
//!
//! On the first turn all tool schemas are sent so the model can start calling them.
//! After the first turn only tools that have been invoked are sent as full API
//! definitions, saving hundreds of tokens per request.

use std::collections::HashSet;
use std::sync::Arc;

use crate::llm::tool::ToolSchema;

/// Strategy that controls when a tool schema is included in the API request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionStrategy {
    /// First turn: send all tools so the model can discover them.
    SendAll,
    /// Subsequent turns: only send tools that have been used.
    SendExpanded,
}

/// Progressive tool view that starts with all tools and narrows to only
/// the tools the model actually uses.
///
/// Lifecycle:
/// 1. Created per-session with the full tool set.
/// 2. First turn: `tool_schemas()` returns all tools (`SendAll`).
/// 3. After each turn: caller marks invoked tools via `note_invoked()`.
/// 4. Subsequent turns: `tool_schemas()` returns only expanded tools.
///
/// This mirrors crabclaw's `ProgressiveToolView` but adapted for bendclaw's
/// shared-storage, server-side architecture.
pub struct ProgressiveToolView {
    all_tools: Arc<Vec<ToolSchema>>,
    expanded: HashSet<String>,
    strategy: ExpansionStrategy,
}

impl ProgressiveToolView {
    /// Create a new progressive view over the given tool set.
    pub fn new(tools: Arc<Vec<ToolSchema>>) -> Self {
        Self {
            all_tools: tools,
            expanded: HashSet::new(),
            strategy: ExpansionStrategy::SendAll,
        }
    }

    /// Return tool schemas for the current API request.
    ///
    /// - `SendAll`: returns all tools (first turn).
    /// - `SendExpanded`: returns only tools that have been invoked.
    pub fn tool_schemas(&self) -> Vec<ToolSchema> {
        match self.strategy {
            ExpansionStrategy::SendAll => self.all_tools.as_ref().clone(),
            ExpansionStrategy::SendExpanded => {
                if self.expanded.is_empty() {
                    // Fallback: if nothing expanded yet, send all.
                    return self.all_tools.as_ref().clone();
                }
                self.all_tools
                    .iter()
                    .filter(|t| self.expanded.contains(&t.function.name))
                    .cloned()
                    .collect()
            }
        }
    }

    /// Mark a tool as invoked. Expands it for future turns.
    pub fn note_invoked(&mut self, name: &str) {
        if self.has_tool(name) {
            self.expanded.insert(name.to_string());
        }
    }

    /// Mark multiple tools as invoked.
    pub fn note_invoked_batch(&mut self, names: &[String]) {
        for name in names {
            self.note_invoked(name);
        }
    }

    /// Transition from `SendAll` to `SendExpanded` after the first turn.
    ///
    /// Should be called after the first LLM turn completes.
    pub fn advance(&mut self) {
        if self.strategy == ExpansionStrategy::SendAll {
            self.strategy = ExpansionStrategy::SendExpanded;
        }
    }

    /// Current expansion strategy.
    pub fn strategy(&self) -> ExpansionStrategy {
        self.strategy
    }

    /// Number of currently expanded tools.
    pub fn expanded_count(&self) -> usize {
        self.expanded.len()
    }

    /// Total number of registered tools.
    pub fn total_count(&self) -> usize {
        self.all_tools.len()
    }

    /// Names of expanded tools (sorted).
    pub fn expanded_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.expanded.iter().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Reset to initial state (all tools, SendAll strategy).
    pub fn reset(&mut self) {
        self.expanded.clear();
        self.strategy = ExpansionStrategy::SendAll;
    }

    fn has_tool(&self, name: &str) -> bool {
        self.all_tools.iter().any(|t| t.function.name == name)
    }
}
