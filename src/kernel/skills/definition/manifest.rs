//! Credential declarations for hub skills (manifest.json).

use serde::Deserialize;
use serde::Serialize;

fn default_true() -> bool {
    true
}

/// A single credential required by a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSpec {
    /// Environment variable name injected at runtime.
    pub env: String,
    /// Human-readable label for the UI.
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub secret: bool,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub setup_url: Option<String>,
    /// Regex pattern for client-side validation.
    #[serde(default)]
    pub validation: Option<String>,
}

/// Parsed `manifest.json` from a skill directory.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillManifest {
    #[serde(default)]
    pub credentials: Vec<CredentialSpec>,
}
