use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use super::build::build_prompt;
use super::model::*;
use super::prompt_contract::PromptResolver;
use crate::base::Result;
use crate::llm::tool::ToolSchema;

pub struct LocalPromptResolver {
    seed: PromptSeed,
    tools: Arc<Vec<ToolSchema>>,
    cwd: PathBuf,
}

impl LocalPromptResolver {
    pub fn new(seed: PromptSeed, tools: Arc<Vec<ToolSchema>>, cwd: PathBuf) -> Self {
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
