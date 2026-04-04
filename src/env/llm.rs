use serde::Deserialize;
use serde::Serialize;

use crate::error::BendclawError;
use crate::error::Result;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Anthropic,
    OpenAi,
}

impl ProviderKind {
    pub fn from_str_loose(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" => Ok(Self::Anthropic),
            "openai" => Ok(Self::OpenAi),
            other => Err(BendclawError::Env(format!("unknown provider: {other}"))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: ProviderKind,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
}

pub fn default_model(provider: &ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Anthropic => "claude-sonnet-4-20250514",
        ProviderKind::OpenAi => "gpt-4o",
    }
}
