use std::path::PathBuf;

use evot_engine::ThinkingLevel;
use indexmap::IndexMap;
use serde::Deserialize;
use serde::Serialize;

use crate::conf::paths;
use crate::error::EvotError;
use crate::error::Result;
use crate::gateway::channels::feishu::FeishuChannelConfig;

// ---------------------------------------------------------------------------
// Protocol — determines which LLM provider implementation to use
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Anthropic,
    OpenAi,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic => write!(f, "anthropic"),
            Self::OpenAi => write!(f, "openai"),
        }
    }
}

/// Infer protocol from provider name. Only "anthropic" maps to Anthropic;
/// everything else defaults to OpenAI-compatible.
pub fn infer_protocol(name: &str) -> Protocol {
    if name == "anthropic" {
        Protocol::Anthropic
    } else {
        Protocol::OpenAi
    }
}

pub fn parse_protocol(value: &str) -> Result<Protocol> {
    match value.to_lowercase().as_str() {
        "anthropic" => Ok(Protocol::Anthropic),
        "openai" => Ok(Protocol::OpenAi),
        other => Err(EvotError::Conf(format!(
            "unknown protocol: {other} (valid: anthropic, openai)"
        ))),
    }
}

// ---------------------------------------------------------------------------
// ProviderProfile — static config for one provider
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProviderProfile {
    pub protocol: Protocol,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

// ---------------------------------------------------------------------------
// LlmSelection — which provider is active + runtime overrides
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LlmSelection {
    pub provider: String,
    pub model_override: Option<String>,
    pub thinking_level: ThinkingLevel,
}

impl Default for LlmSelection {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model_override: None,
            thinking_level: ThinkingLevel::Off,
        }
    }
}

// ---------------------------------------------------------------------------
// LlmConfig — resolved runtime config passed to Agent / Engine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: String,
    pub protocol: Protocol,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub thinking_level: ThinkingLevel,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Config {
    pub id: Option<String>,
    pub llm: LlmSelection,
    pub providers: IndexMap<String, ProviderProfile>,
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub channels: ChannelsConfig,
    pub sandbox: SandboxConfig,
    pub skills_dirs: Vec<PathBuf>,
    /// The env file path actually used during config loading.
    pub env_file_path: PathBuf,
}

impl Config {
    pub fn new(state_root: PathBuf) -> Self {
        Self {
            id: None,
            llm: LlmSelection::default(),
            providers: IndexMap::new(),
            server: ServerConfig::default(),
            storage: StorageConfig::fs(state_root),
            channels: ChannelsConfig::default(),
            sandbox: SandboxConfig::default(),
            skills_dirs: Vec::new(),
            env_file_path: PathBuf::new(),
        }
    }

    /// Resolve the active provider into a runtime LlmConfig.
    pub fn active_llm(&self) -> Result<LlmConfig> {
        let profile = self.providers.get(&self.llm.provider).ok_or_else(|| {
            EvotError::Conf(format!(
                "provider '{}' not found, available: {}",
                self.llm.provider,
                self.providers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        Ok(LlmConfig {
            provider: self.llm.provider.clone(),
            protocol: profile.protocol.clone(),
            api_key: profile.api_key.clone(),
            base_url: profile.base_url.clone(),
            model: self
                .llm
                .model_override
                .clone()
                .unwrap_or_else(|| profile.model.clone()),
            thinking_level: self.llm.thinking_level,
        })
    }

    /// Parse a model spec and return (provider_name, optional_model_override).
    ///
    /// Formats:
    /// - `"deepseek-chat"` — find first provider whose model matches
    /// - `"openrouter:google/gemini-2.5-pro"` — exact provider + model override
    pub fn resolve_model_spec(&self, spec: &str) -> Result<(String, Option<String>)> {
        if let Some((provider, model)) = spec.split_once(':') {
            if model.is_empty() {
                return Err(EvotError::Conf(format!(
                    "empty model in spec '{spec}', expected provider:model"
                )));
            }
            if !self.providers.contains_key(provider) {
                return Err(EvotError::Conf(format!(
                    "provider '{}' not found, available: {}",
                    provider,
                    self.providers
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            Ok((provider.to_string(), Some(model.to_string())))
        } else {
            let found = self
                .providers
                .iter()
                .find(|(_, p)| p.model == spec)
                .map(|(name, _)| name.clone());
            match found {
                Some(name) => Ok((name, None)),
                None => Err(EvotError::Conf(format!(
                    "no provider with model '{}', available: {}",
                    spec,
                    self.providers
                        .iter()
                        .map(|(n, p)| format!("{}:{}", n, p.model))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))),
            }
        }
    }

    /// Apply `--model` CLI argument. Must be called before `validate()`.
    pub fn with_model(mut self, model: Option<String>) -> Result<Self> {
        let Some(value) = model else {
            return Ok(self);
        };
        let (provider, model_override) = self.resolve_model_spec(&value)?;
        self.llm.provider = provider;
        self.llm.model_override = model_override;
        Ok(self)
    }

    pub fn load() -> Result<Self> {
        super::load::load_config_inner(None)
    }

    pub fn load_with_env_file(env_file: Option<&str>) -> Result<Self> {
        super::load::load_config_inner(env_file)
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.server.port = port;
        self
    }

    pub fn validate(&self) -> Result<()> {
        let profile = self.providers.get(&self.llm.provider).ok_or_else(|| {
            EvotError::Conf(format!("provider '{}' not found", self.llm.provider))
        })?;
        if profile.api_key.is_empty() {
            return Err(EvotError::Conf(format!(
                "{}.api_key not set (env file: {})",
                self.llm.provider,
                self.env_file_path.display()
            )));
        }
        if profile.base_url.is_empty() {
            return Err(EvotError::Conf(format!(
                "{}.base_url not set (env file: {})",
                self.llm.provider,
                self.env_file_path.display()
            )));
        }
        if profile.model.is_empty() && self.llm.model_override.is_none() {
            return Err(EvotError::Conf(format!(
                "{}.model not set (env file: {})",
                self.llm.provider,
                self.env_file_path.display()
            )));
        }

        match self.storage.backend {
            StorageBackend::Fs => {
                if self.storage.fs.root_dir.as_os_str().is_empty() {
                    return Err(EvotError::Conf("storage.fs.root_dir not set".into()));
                }
            }
            StorageBackend::Cloud => {
                if self.storage.cloud.endpoint.is_empty() {
                    return Err(EvotError::Conf("storage.cloud.endpoint not set".into()));
                }
                if self.storage.cloud.api_key.is_empty() {
                    return Err(EvotError::Conf("storage.cloud.api_key not set".into()));
                }
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Server / Storage / Channels / Sandbox — unchanged
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8082,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub backend: StorageBackend,
    pub fs: FsStorageConfig,
    pub cloud: CloudStorageConfig,
}

impl StorageConfig {
    pub fn fs(root_dir: PathBuf) -> Self {
        Self {
            backend: StorageBackend::Fs,
            fs: FsStorageConfig { root_dir },
            cloud: CloudStorageConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    #[default]
    Fs,
    Cloud,
}

#[derive(Debug, Clone)]
pub struct FsStorageConfig {
    pub root_dir: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct CloudStorageConfig {
    pub endpoint: String,
    pub api_key: String,
    pub workspace: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn thinking_level_from_str(value: &str) -> Result<ThinkingLevel> {
    match value.to_lowercase().as_str() {
        "off" => Ok(ThinkingLevel::Off),
        "minimal" => Ok(ThinkingLevel::Minimal),
        "low" => Ok(ThinkingLevel::Low),
        "medium" => Ok(ThinkingLevel::Medium),
        "high" => Ok(ThinkingLevel::High),
        other => Err(EvotError::Conf(format!(
            "unknown thinking level: {other} (valid: off, minimal, low, medium, high)"
        ))),
    }
}

pub fn default_config() -> Result<Config> {
    Ok(Config::new(paths::state_root_dir()?))
}

#[derive(Debug, Clone, Default)]
pub struct ChannelsConfig {
    pub feishu: Option<FeishuChannelConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub allowed_dirs: Vec<PathBuf>,
}
