use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::conf::paths;
use crate::error::BendclawError;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct Config {
    pub llm: LlmSelection,
    pub anthropic: ProviderConfig,
    pub openai: ProviderConfig,
    pub server: ServerConfig,
    pub store: StoreConfig,
}

impl Config {
    pub fn new(state_root: PathBuf) -> Self {
        Self {
            llm: LlmSelection::default(),
            anthropic: ProviderConfig::anthropic(),
            openai: ProviderConfig::openai(),
            server: ServerConfig::default(),
            store: StoreConfig::fs(state_root),
        }
    }

    pub fn active_llm(&self) -> LlmConfig {
        let provider = self.llm.provider.clone();
        let config = self.provider_config(&provider).clone();

        LlmConfig {
            provider,
            api_key: config.api_key,
            base_url: config.base_url,
            model: config.model,
        }
    }

    pub fn provider_config(&self, provider: &ProviderKind) -> &ProviderConfig {
        match provider {
            ProviderKind::Anthropic => &self.anthropic,
            ProviderKind::OpenAi => &self.openai,
        }
    }

    pub fn provider_config_mut(&mut self, provider: &ProviderKind) -> &mut ProviderConfig {
        match provider {
            ProviderKind::Anthropic => &mut self.anthropic,
            ProviderKind::OpenAi => &mut self.openai,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let llm = self.active_llm();
        if llm.api_key.is_empty() {
            return Err(BendclawError::Conf("active llm api_key not set".into()));
        }

        match self.store.backend {
            StoreBackend::Fs => {
                if self.store.fs.root_dir.as_os_str().is_empty() {
                    return Err(BendclawError::Conf("store.fs.root_dir not set".into()));
                }
            }
            StoreBackend::Cloud => {
                if self.store.cloud.endpoint.is_empty() {
                    return Err(BendclawError::Conf("store.cloud.endpoint not set".into()));
                }
                if self.store.cloud.api_key.is_empty() {
                    return Err(BendclawError::Conf("store.cloud.api_key not set".into()));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ConfigOverrides {
    pub model: Option<String>,
    pub port: Option<u16>,
}

impl ConfigOverrides {
    pub fn new(model: Option<String>, port: Option<u16>) -> Self {
        Self { model, port }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LlmSelection {
    pub provider: ProviderKind,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: ProviderKind,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
}

impl ProviderConfig {
    pub fn anthropic() -> Self {
        Self {
            api_key: String::new(),
            base_url: None,
            model: default_model(&ProviderKind::Anthropic).to_string(),
        }
    }

    pub fn openai() -> Self {
        Self {
            api_key: String::new(),
            base_url: None,
            model: default_model(&ProviderKind::OpenAi).to_string(),
        }
    }
}

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
pub struct StoreConfig {
    pub backend: StoreBackend,
    pub fs: FsStoreConfig,
    pub cloud: CloudStoreConfig,
}

impl StoreConfig {
    pub fn fs(root_dir: PathBuf) -> Self {
        Self {
            backend: StoreBackend::Fs,
            fs: FsStoreConfig { root_dir },
            cloud: CloudStoreConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreBackend {
    #[default]
    Fs,
    Cloud,
}

#[derive(Debug, Clone)]
pub struct FsStoreConfig {
    pub root_dir: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct CloudStoreConfig {
    pub endpoint: String,
    pub api_key: String,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Anthropic,
    OpenAi,
}

impl ProviderKind {
    pub fn from_str_loose(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "anthropic" => Ok(Self::Anthropic),
            "openai" => Ok(Self::OpenAi),
            other => Err(BendclawError::Conf(format!("unknown provider: {other}"))),
        }
    }
}

pub fn default_model(provider: &ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Anthropic => "claude-sonnet-4-20250514",
        ProviderKind::OpenAi => "gpt-4o",
    }
}

pub fn default_config() -> Result<Config> {
    Ok(Config::new(paths::state_root_dir()?))
}
