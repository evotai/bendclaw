//! Skill execution: trait and output types.

use std::fmt;

use serde::Deserialize;
use serde::Serialize;

use crate::types::Result;

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

/// Executes a skill by name.
#[async_trait::async_trait]
pub trait SkillExecutor: Send + Sync + 'static {
    async fn execute(&self, skill_name: &str, args: &[String]) -> Result<SkillOutput>;
}
