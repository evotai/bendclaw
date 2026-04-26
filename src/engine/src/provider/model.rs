//! Model configuration and provider compatibility flags.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

/// Which API protocol a model uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProtocol {
    AnthropicMessages,
    OpenAiCompletions,
    BedrockConverseStream,
}

impl std::fmt::Display for ApiProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnthropicMessages => write!(f, "anthropic_messages"),
            Self::OpenAiCompletions => write!(f, "openai_completions"),
            Self::BedrockConverseStream => write!(f, "bedrock_converse_stream"),
        }
    }
}

/// Cost per million tokens (input/output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostConfig {
    pub input_per_million: f64,
    pub output_per_million: f64,
    #[serde(default)]
    pub cache_read_per_million: f64,
    #[serde(default)]
    pub cache_write_per_million: f64,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            input_per_million: 0.0,
            output_per_million: 0.0,
            cache_read_per_million: 0.0,
            cache_write_per_million: 0.0,
        }
    }
}

/// How a provider handles the `max_tokens` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MaxTokensField {
    #[default]
    MaxTokens,
    MaxCompletionTokens,
}

/// How a provider formats thinking/reasoning output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingFormat {
    #[default]
    OpenAi,
    Xai,
    Qwen,
}

/// Bitflag set of OpenAI-compatible provider capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompatCaps(u16);

impl CompatCaps {
    pub const NONE: Self = Self(0);
    pub const STORE: Self = Self(1 << 0);
    pub const DEVELOPER_ROLE: Self = Self(1 << 1);
    pub const REASONING_EFFORT: Self = Self(1 << 2);
    pub const USAGE_IN_STREAMING: Self = Self(1 << 3);
    pub const TOOL_RESULT_NAME: Self = Self(1 << 4);
    pub const ASSISTANT_AFTER_TOOL_RESULT: Self = Self(1 << 5);
    pub const REASONING_CONTENT_REQUIRED: Self = Self(1 << 6);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for CompatCaps {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for CompatCaps {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl Serialize for CompatCaps {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let all = [
            (Self::STORE, "store"),
            (Self::DEVELOPER_ROLE, "developer_role"),
            (Self::REASONING_EFFORT, "reasoning_effort"),
            (Self::USAGE_IN_STREAMING, "usage_in_streaming"),
            (Self::TOOL_RESULT_NAME, "tool_result_name"),
            (
                Self::ASSISTANT_AFTER_TOOL_RESULT,
                "assistant_after_tool_result",
            ),
            (
                Self::REASONING_CONTENT_REQUIRED,
                "reasoning_content_required",
            ),
        ];
        let count = all.iter().filter(|(f, _)| self.contains(*f)).count();
        let mut seq = serializer.serialize_seq(Some(count))?;
        for (flag, name) in &all {
            if self.contains(*flag) {
                seq.serialize_element(name)?;
            }
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for CompatCaps {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let names: Vec<String> = Vec::deserialize(deserializer)?;
        let mut caps = Self::NONE;
        for name in &names {
            caps |= match name.as_str() {
                "store" => Self::STORE,
                "developer_role" => Self::DEVELOPER_ROLE,
                "reasoning_effort" => Self::REASONING_EFFORT,
                "usage_in_streaming" => Self::USAGE_IN_STREAMING,
                "tool_result_name" => Self::TOOL_RESULT_NAME,
                "assistant_after_tool_result" => Self::ASSISTANT_AFTER_TOOL_RESULT,
                "reasoning_content_required" => Self::REASONING_CONTENT_REQUIRED,
                other => {
                    return Err(serde::de::Error::unknown_variant(other, &[
                        "store",
                        "developer_role",
                        "reasoning_effort",
                        "usage_in_streaming",
                        "tool_result_name",
                        "assistant_after_tool_result",
                        "reasoning_content_required",
                    ]))
                }
            };
        }
        Ok(caps)
    }
}

/// Compatibility flags for OpenAI-compatible providers.
/// Different providers have different quirks even though they share the same base API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiCompat {
    /// Provider capabilities/quirks.
    #[serde(default)]
    pub caps: CompatCaps,
    /// Which field name to use for max tokens.
    pub max_tokens_field: MaxTokensField,
    /// How thinking/reasoning content is formatted in streaming.
    pub thinking_format: ThinkingFormat,
}

impl Default for OpenAiCompat {
    fn default() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING,
            max_tokens_field: MaxTokensField::MaxTokens,
            thinking_format: ThinkingFormat::OpenAi,
        }
    }
}

impl OpenAiCompat {
    pub fn has_cap(&self, cap: CompatCaps) -> bool {
        self.caps.contains(cap)
    }

    /// Compat flags for native OpenAI.
    pub fn openai() -> Self {
        Self {
            caps: CompatCaps::STORE
                | CompatCaps::DEVELOPER_ROLE
                | CompatCaps::REASONING_EFFORT
                | CompatCaps::USAGE_IN_STREAMING,
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..Default::default()
        }
    }

    /// Compat flags for xAI (Grok).
    pub fn xai() -> Self {
        Self {
            thinking_format: ThinkingFormat::Xai,
            ..Default::default()
        }
    }

    /// Compat flags for Groq.
    pub fn groq() -> Self {
        Self::default()
    }

    /// Compat flags for Cerebras.
    pub fn cerebras() -> Self {
        Self::default()
    }

    /// Compat flags for OpenRouter.
    pub fn openrouter() -> Self {
        Self {
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..Default::default()
        }
    }

    /// Compat flags for Mistral.
    pub fn mistral() -> Self {
        Self {
            max_tokens_field: MaxTokensField::MaxTokens,
            ..Default::default()
        }
    }

    /// Compat flags for DeepSeek.
    pub fn deepseek() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING | CompatCaps::REASONING_CONTENT_REQUIRED,
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..Default::default()
        }
    }

    /// Compat flags for Z.ai (Zhipu AI).
    pub fn zai() -> Self {
        Self::default()
    }

    /// Compat flags for MiniMax.
    pub fn minimax() -> Self {
        Self::default()
    }
}

/// Full model configuration. Knows everything needed to make API calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model identifier sent to the API (e.g. "gpt-4o", "claude-sonnet-4-20250514").
    pub id: String,
    /// Human-friendly name.
    pub name: String,
    /// Which API protocol to use.
    pub api: ApiProtocol,
    /// Provider name (e.g. "openai", "anthropic", "xai").
    pub provider: String,
    /// Base URL for API requests (without trailing slash).
    pub base_url: String,
    /// Whether this model supports reasoning/thinking.
    pub reasoning: bool,
    /// Context window size in tokens.
    pub context_window: u32,
    /// Default max output tokens.
    pub max_tokens: u32,
    /// Cost configuration.
    #[serde(default)]
    pub cost: CostConfig,
    /// Additional headers to send with requests.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// OpenAI-compat quirk flags (only for OpenAiCompletions protocol).
    #[serde(default)]
    pub compat: Option<OpenAiCompat>,
}

impl ModelConfig {
    /// Create a new Anthropic model config.
    pub fn anthropic(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::AnthropicMessages,
            provider: "anthropic".into(),
            base_url: "https://api.anthropic.com".into(),
            reasoning: false,
            context_window: 200_000,
            max_tokens: 8192,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: None,
        }
    }

    /// Create a new OpenAI model config.
    pub fn openai(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            reasoning: false,
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::openai()),
        }
    }

    /// Create a config for a local OpenAI-compatible server (LM Studio, Ollama, etc.).
    /// No API key required — sends an empty Bearer token.
    pub fn local(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            id: model_id.into(),
            name: "Local Model".into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "local".into(),
            base_url: base_url.into(),
            reasoning: false,
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::default()),
        }
    }

    /// Create a new Z.ai (Zhipu AI) model config.
    ///
    /// Models: `glm-4.7`, `glm-4.5-air`, `glm-5`, etc.
    pub fn zai(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "zai".into(),
            base_url: "https://api.z.ai/api/paas/v4".into(),
            reasoning: false,
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::zai()),
        }
    }

    /// Create a new MiniMax model config.
    ///
    /// Models: `MiniMax-Text-01`, `MiniMax-M1`, etc.
    pub fn minimax(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "minimax".into(),
            base_url: "https://api.minimaxi.chat/v1".into(),
            reasoning: false,
            context_window: 1_000_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::minimax()),
        }
    }

    /// Create a new xAI (Grok) model config.
    ///
    /// Models: `grok-3-mini`, `grok-3`, etc.
    pub fn xai(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "xai".into(),
            base_url: "https://api.x.ai/v1".into(),
            reasoning: false,
            context_window: 131_072,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::xai()),
        }
    }

    /// Create a new Groq model config.
    ///
    /// Models: `llama-3.3-70b-versatile`, `mixtral-8x7b-32768`, etc.
    pub fn groq(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            reasoning: false,
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::groq()),
        }
    }

    /// Create a new DeepSeek model config.
    ///
    /// Models: `deepseek-chat`, `deepseek-reasoner`, etc.
    pub fn deepseek(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            reasoning: false,
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::deepseek()),
        }
    }

    /// Create a new Mistral model config.
    ///
    /// Models: `mistral-large-latest`, `mistral-small-latest`, etc.
    pub fn mistral(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            api: ApiProtocol::OpenAiCompletions,
            provider: "mistral".into(),
            base_url: "https://api.mistral.ai/v1".into(),
            reasoning: false,
            context_window: 128_000,
            max_tokens: 4096,
            cost: CostConfig::default(),
            headers: HashMap::new(),
            compat: Some(OpenAiCompat::mistral()),
        }
    }
}
