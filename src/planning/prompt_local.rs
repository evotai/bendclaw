use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use super::prompt_contract::PromptResolver;
use super::prompt_model::*;
use super::prompt_renderer::build_prompt;
use crate::tools::definition::tool_definition::ToolDefinition;
use crate::types::Result;

pub struct LocalPromptResolver {
    seed: PromptSeed,
    tools: Arc<Vec<ToolDefinition>>,
    cwd: PathBuf,
}

impl LocalPromptResolver {
    pub fn new(seed: PromptSeed, tools: Arc<Vec<ToolDefinition>>, cwd: PathBuf) -> Self {
        Self { seed, tools, cwd }
    }
}

#[async_trait]
impl PromptResolver for LocalPromptResolver {
    async fn resolve(&self, meta: &PromptRequestMeta) -> Result<String> {
        Ok(build_prompt(PromptInputs {
            seed: self.seed.clone(),
            tools: self.tools.clone(),
            cwd: self.cwd.clone(),
            system_overlay: meta.system_overlay.clone(),
            skill_overlay: meta.skill_overlay.clone(),
            memory_recall: None,
            cluster_info: None,
            recent_errors: None,
            session_state: None,
            channel_type: meta.channel_type.clone(),
            channel_chat_id: meta.channel_chat_id.clone(),
            runtime_override: None,
        }))
    }
}
