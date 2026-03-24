//! Agent runtime configuration types.

use serde::Deserialize;
use serde::Serialize;

use crate::config::WorkspaceConfig;
use crate::config::DEFAULT_DB_PREFIX;

/// Everything needed to build and run the agent runtime.
#[derive(Debug, Clone, Serialize)]
pub struct AgentConfig {
    pub node_id: String,
    pub databend_api_base_url: String,
    pub databend_api_token: String,
    pub databend_warehouse: String,
    pub db_prefix: String,
    pub max_iterations: u32,
    pub max_context_tokens: usize,
    pub max_duration_secs: u64,
    pub workspace: WorkspaceConfig,
    pub checkpoint: CheckpointConfig,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            databend_api_base_url: String::new(),
            databend_api_token: String::new(),
            databend_warehouse: "default".to_string(),
            db_prefix: DEFAULT_DB_PREFIX.to_string(),
            max_iterations: 20,
            max_context_tokens: 250_000,
            max_duration_secs: 300,
            workspace: WorkspaceConfig::default(),
            checkpoint: CheckpointConfig::default(),
        }
    }
}

/// Configuration for the pre-compaction checkpoint step.
///
/// Before conversation context is summarized/discarded, the checkpoint
/// prompts the agent to persist important state to memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    #[serde(default = "default_checkpoint_enabled")]
    pub enabled: bool,
    /// Trigger when remaining context budget falls below this percentage.
    #[serde(default = "default_checkpoint_threshold")]
    pub threshold: usize,
    #[serde(default = "default_checkpoint_prompt")]
    pub prompt: String,
}

fn default_checkpoint_enabled() -> bool {
    true
}
fn default_checkpoint_threshold() -> usize {
    20
}
fn default_checkpoint_prompt() -> String {
    "Checkpoint: Store important information to memory now. \
     Focus on: user preferences, key decisions, facts. \
     Reply 'OK' if nothing to store."
        .to_string()
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            enabled: default_checkpoint_enabled(),
            threshold: default_checkpoint_threshold(),
            prompt: default_checkpoint_prompt(),
        }
    }
}
