use serde::Deserialize;
use serde::Serialize;

/// Long-lived behavioral rules for a session — CLAUDE.md equivalent.
///
/// Loaded at session start from persistent store. Updated by explicit user
/// action, not by run execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionRules {
    /// Raw CLAUDE.md content (user-level + agent-level merged).
    #[serde(default)]
    pub rules_text: String,
    /// User preferences extracted from CLAUDE.md.
    #[serde(default)]
    pub preferences: Vec<String>,
    /// Project conventions extracted from CLAUDE.md.
    #[serde(default)]
    pub conventions: Vec<String>,
}

impl SessionRules {
    pub fn is_empty(&self) -> bool {
        self.rules_text.is_empty() && self.preferences.is_empty() && self.conventions.is_empty()
    }
}
