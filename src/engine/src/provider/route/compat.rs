use serde::Deserialize;
use serde::Serialize;

/// How a provider handles the output-token field.
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
    OpenRouter,
    DeepSeek,
    Xai,
    Qwen,
}

/// Capabilities of an OpenAI-compatible transport.
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
    pub const PROMPT_CACHE_KEY: Self = Self(1 << 7);

    const ALL: [(Self, &'static str); 8] = [
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
        (Self::PROMPT_CACHE_KEY, "prompt_cache_key"),
    ];

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find_map(|(cap, candidate)| (*candidate == name).then_some(*cap))
    }

    /// Stable config names for the enabled capabilities.
    pub fn names(self) -> Vec<&'static str> {
        Self::ALL
            .iter()
            .filter_map(|(cap, name)| self.contains(*cap).then_some(*name))
            .collect()
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

        let count = Self::ALL
            .iter()
            .filter(|(cap, _)| self.contains(*cap))
            .count();
        let mut seq = serializer.serialize_seq(Some(count))?;
        for (cap, name) in Self::ALL {
            if self.contains(cap) {
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
        for name in names {
            let Some(cap) = Self::from_name(&name) else {
                let valid = Self::ALL
                    .iter()
                    .map(|(_, name)| *name)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(serde::de::Error::custom(format!(
                    "unknown compatibility capability `{name}`; expected one of: {valid}"
                )));
            };
            caps |= cap;
        }
        Ok(caps)
    }
}

/// Compatibility metadata for an OpenAI-compatible endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiCompat {
    #[serde(default)]
    pub caps: CompatCaps,
    pub max_tokens_field: MaxTokensField,
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

    pub fn for_provider(provider: &str) -> Self {
        match provider {
            "openai" => Self::openai(),
            "deepseek" => Self::deepseek(),
            "xai" => Self::xai(),
            "grok" => Self::grok_cli(),
            "groq" => Self::groq(),
            "cerebras" => Self::cerebras(),
            "openrouter" => Self::openrouter(),
            "moonshotai" | "moonshotai-cn" => Self::moonshot(),
            "mistral" => Self::mistral(),
            "zai" => Self::zai(),
            "minimax" => Self::minimax(),
            _ => Self::default(),
        }
    }

    pub fn openai() -> Self {
        Self {
            caps: CompatCaps::STORE
                | CompatCaps::DEVELOPER_ROLE
                | CompatCaps::REASONING_EFFORT
                | CompatCaps::USAGE_IN_STREAMING
                | CompatCaps::PROMPT_CACHE_KEY,
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..Default::default()
        }
    }

    pub fn xai() -> Self {
        Self {
            thinking_format: ThinkingFormat::Xai,
            ..Default::default()
        }
    }

    /// llmproxy Grok CLI Chat Completions adapter.
    pub fn grok_cli() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING | CompatCaps::REASONING_EFFORT,
            ..Default::default()
        }
    }

    pub fn groq() -> Self {
        Self::default()
    }

    pub fn cerebras() -> Self {
        Self::default()
    }

    pub fn openrouter() -> Self {
        Self {
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            thinking_format: ThinkingFormat::OpenRouter,
            ..Default::default()
        }
    }

    pub fn mistral() -> Self {
        Self {
            max_tokens_field: MaxTokensField::MaxTokens,
            ..Default::default()
        }
    }

    pub fn deepseek() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING
                | CompatCaps::REASONING_EFFORT
                | CompatCaps::REASONING_CONTENT_REQUIRED,
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            thinking_format: ThinkingFormat::DeepSeek,
        }
    }

    pub fn moonshot() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING | CompatCaps::REASONING_CONTENT_REQUIRED,
            max_tokens_field: MaxTokensField::MaxTokens,
            thinking_format: ThinkingFormat::DeepSeek,
        }
    }

    pub fn zai() -> Self {
        Self {
            caps: CompatCaps::USAGE_IN_STREAMING | CompatCaps::REASONING_EFFORT,
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..Default::default()
        }
    }

    pub fn minimax() -> Self {
        Self::default()
    }
}
