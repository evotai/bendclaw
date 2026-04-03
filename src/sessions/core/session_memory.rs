use serde::Deserialize;
use serde::Serialize;

/// Accumulated knowledge from runs — facts learned, decisions made.
///
/// Written by `memory_writeback` at run end. Read by `session_guidance`
/// (planning) to inject into prompt.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMemory {
    /// Raw MEMORY.md content.
    #[serde(default)]
    pub memory_text: String,
    /// Individual fact entries extracted from memory.
    #[serde(default)]
    pub facts: Vec<MemoryFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub content: String,
    pub source_run_id: String,
    pub created_at: String,
}

impl SessionMemory {
    pub fn is_empty(&self) -> bool {
        self.memory_text.is_empty() && self.facts.is_empty()
    }
}
