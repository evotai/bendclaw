use async_trait::async_trait;

use crate::types::ApiToolParam;
use crate::types::Message;
use crate::types::SystemBlock;
use crate::types::ThinkingConfig;
use crate::types::Usage;

/// API type identifier.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiType {
    AnthropicMessages,
    OpenAICompletions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
}

impl ProviderKind {
    pub fn api_type(&self) -> ApiType {
        match self {
            Self::Anthropic => ApiType::AnthropicMessages,
            Self::OpenAi => ApiType::OpenAICompletions,
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Anthropic => "claude-sonnet-4-6-20250514",
            Self::OpenAi => "gpt-4o",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub message: Message,
    pub usage: Usage,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderRequest<'a> {
    pub model: &'a str,
    pub max_tokens: u64,
    pub messages: &'a [Message],
    pub system: Option<Vec<SystemBlock>>,
    pub tools: Option<Vec<ApiToolParam>>,
    pub thinking: Option<ThinkingConfig>,
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    fn api_type(&self) -> ApiType;

    async fn create_message(
        &self,
        request: ProviderRequest<'_>,
    ) -> Result<ProviderResponse, super::ApiError>;
}
