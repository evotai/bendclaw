use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub node_id: String,
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub llm: LlmConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub bind_addr: String,
}

#[derive(Debug, Deserialize)]
pub struct StorageConfig {
    pub databend_api_base_url: String,
    pub databend_api_token: String,
}

#[derive(Debug, Deserialize)]
pub struct LlmConfig {
    pub providers: Vec<LlmProviderConfig>,
}

#[derive(Debug, Deserialize)]
pub struct LlmProviderConfig {
    pub name: String,
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl Config {
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
