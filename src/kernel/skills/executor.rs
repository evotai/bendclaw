//! Skill execution: trait, output types, and arg parsing.

use std::fmt;

use serde::Deserialize;
use serde::Serialize;

use crate::base::Result;

// ── Output types ──────────────────────────────────────────────────────────────

/// Output from a successful (or gracefully failed) skill execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutput {
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl SkillOutput {
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Structured error returned when skill execution fails at the executor level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillError {
    pub skill_name: String,
    pub message: String,
    pub exit_code: Option<i32>,
}

impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "skill '{}': {}", self.skill_name, self.message)
    }
}

impl std::error::Error for SkillError {}

/// Parse JSON tool call arguments into CLI args for the skill executor.
pub fn parse_skill_args(skill_name: &str, arguments: &str) -> Vec<String> {
    let parsed: serde_json::Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(skill = skill_name, error = %e, "failed to parse tool arguments");
            return vec![];
        }
    };

    let mut args = Vec::new();
    if let Some(obj) = parsed.as_object() {
        for (key, value) in obj {
            args.push(format!("--{key}"));
            match value {
                serde_json::Value::String(s) => args.push(s.clone()),
                other => args.push(other.to_string()),
            }
        }
    }
    args
}

// ── SkillExecutor trait ────────────────────────────────────────────────────────

/// Executes a skill by name.
#[async_trait::async_trait]
pub trait SkillExecutor: Send + Sync + 'static {
    async fn execute(&self, skill_name: &str, args: &[String]) -> Result<SkillOutput>;
}
