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
    pub memory: MemoryConfig,
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
            memory: MemoryConfig::default(),
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

/// Configuration for the shared memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Whether the memory system is enabled.
    #[serde(default = "default_memory_enabled")]
    pub enabled: bool,
    /// Enable pre-compaction memory extraction.
    #[serde(default = "default_memory_extract")]
    pub extract: bool,
    /// Enable memory recall injection into prompts.
    #[serde(default = "default_memory_recall")]
    pub recall: bool,
    /// Max characters for prompt recall injection.
    #[serde(default = "default_memory_recall_budget")]
    pub recall_budget: usize,
}

fn default_memory_enabled() -> bool {
    true
}
fn default_memory_extract() -> bool {
    true
}
fn default_memory_recall() -> bool {
    true
}
fn default_memory_recall_budget() -> usize {
    2000
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_memory_enabled(),
            extract: default_memory_extract(),
            recall: default_memory_recall(),
            recall_budget: default_memory_recall_budget(),
        }
    }
}
