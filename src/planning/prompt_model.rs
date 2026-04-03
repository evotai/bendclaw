//! Prompt data types, constants, and utility functions.

use std::path::PathBuf;
use std::sync::Arc;

use crate::kernel::tools::definition::tool_definition::ToolDefinition;
use crate::planning::prompt_diagnostics;

const _RECENT_ERRORS_LIMIT: u32 = 5;

// Per-layer max sizes (bytes). Prevents any single layer from bloating the prompt.
pub const MAX_IDENTITY_BYTES: usize = 8_192;
pub const MAX_SOUL_BYTES: usize = 16_384;
pub const MAX_SYSTEM_BYTES: usize = 65_536;
pub const MAX_SKILLS_BYTES: usize = 32_768;
pub const MAX_TOOLS_BYTES: usize = 32_768;
pub const MAX_ERRORS_BYTES: usize = 8_192;
pub const MAX_VARIABLES_BYTES: usize = 16_384;
pub const MAX_RUNTIME_BYTES: usize = 4_096;
pub const MAX_CLUSTER_BYTES: usize = 8_192;
pub const MAX_DIRECTIVE_BYTES: usize = 4_096;

/// Truncate content to `max_bytes` on a char boundary.
pub fn truncate_layer(layer: &str, content: &str, max_bytes: usize, source: &str) -> String {
    let original = content.len();
    if original <= max_bytes {
        return content.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &content[..end];
    let dropped = original - end;
    prompt_diagnostics::log_prompt_layer_truncated(
        layer, original, end, dropped, max_bytes, source,
    );
    format!("{truncated}\n[... truncated at {end}/{original} bytes ...]")
}

/// Replace `{key}` placeholders with values from session state.
pub fn substitute_template(template: &str, state: &serde_json::Value) -> String {
    if !template.contains('{') || state.is_null() {
        return template.to_string();
    }
    let obj = match state.as_object() {
        Some(o) => o,
        None => return template.to_string(),
    };
    let mut result = template.to_string();
    for (key, value) in obj {
        let placeholder = format!("{{{key}}}");
        let replacement = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&placeholder, &replacement);
    }
    result
}

// ─── Types ───

#[derive(Debug, Clone)]
pub struct PromptVariable {
    pub key: String,
    pub value: String,
    pub secret: bool,
}

impl From<crate::storage::dal::variable::record::VariableRecord> for PromptVariable {
    fn from(value: crate::storage::dal::variable::record::VariableRecord) -> Self {
        Self {
            key: value.key,
            value: value.value,
            secret: value.secret,
        }
    }
}

impl From<&crate::kernel::variables::Variable> for PromptVariable {
    fn from(value: &crate::kernel::variables::Variable) -> Self {
        Self {
            key: value.key.clone(),
            value: value.value.clone(),
            secret: value.secret,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptConfig {
    pub system_prompt: String,
    pub identity: String,
    pub soul: String,
    pub token_limit_total: Option<u64>,
    pub token_limit_daily: Option<u64>,
}

impl From<crate::storage::dal::agent_config::record::AgentConfigRecord> for PromptConfig {
    fn from(value: crate::storage::dal::agent_config::record::AgentConfigRecord) -> Self {
        Self {
            system_prompt: value.system_prompt,
            identity: value.identity,
            soul: value.soul,
            token_limit_total: value.token_limit_total,
            token_limit_daily: value.token_limit_daily,
        }
    }
}

/// Static inputs prepared at assembly time. Owned data, no lifetimes.
#[derive(Debug, Clone, Default)]
pub struct PromptSeed {
    pub cached_config: Option<PromptConfig>,
    pub variables: Vec<PromptVariable>,
    pub skill_prompts: Vec<SkillPromptEntry>,
    pub directive_prompt: Option<String>,
}

/// A non-executable skill's metadata for prompt display.
#[derive(Debug, Clone)]
pub struct SkillPromptEntry {
    pub display_name: String,
    pub description: String,
}

/// All inputs needed to build a prompt. Owns everything — no lifetimes.
#[derive(Debug, Clone)]
pub struct PromptInputs {
    pub seed: PromptSeed,
    pub tools: Arc<Vec<ToolDefinition>>,
    pub cwd: PathBuf,
    pub system_overlay: Option<String>,
    pub skill_overlay: Option<String>,
    pub memory_recall: Option<String>,
    pub cluster_info: Option<String>,
    pub recent_errors: Option<String>,
    pub session_state: Option<serde_json::Value>,
    pub channel_type: Option<String>,
    pub channel_chat_id: Option<String>,
    /// When set, replaces the auto-generated runtime context.
    pub runtime_override: Option<String>,
}

/// Neutral prompt request metadata — no dependency on invocation types.
#[derive(Debug, Clone, Default)]
pub struct PromptRequestMeta {
    pub channel_type: Option<String>,
    pub channel_chat_id: Option<String>,
    pub system_overlay: Option<String>,
    pub skill_overlay: Option<String>,
}
