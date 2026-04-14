use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use crate::conf::default_config;
use crate::conf::paths;
use crate::conf::Config;
use crate::conf::ProviderKind;
use crate::conf::StorageBackend;
use crate::error::EvotError;
use crate::error::Result;

const RELEVANT_KEYS: &[&str] = &[
    "EVOT_LLM_PROVIDER",
    "EVOT_THINKING_LEVEL",
    "EVOT_ANTHROPIC_API_KEY",
    "EVOT_ANTHROPIC_BASE_URL",
    "EVOT_ANTHROPIC_MODEL",
    "EVOT_OPENAI_API_KEY",
    "EVOT_OPENAI_BASE_URL",
    "EVOT_OPENAI_MODEL",
    "EVOT_SERVER_HOST",
    "EVOT_SERVER_PORT",
    "EVOT_STORAGE_BACKEND",
    "EVOT_STORAGE_FS_ROOT_DIR",
    "EVOT_STORAGE_CLOUD_ENDPOINT",
    "EVOT_STORAGE_CLOUD_API_KEY",
    "EVOT_STORAGE_CLOUD_WORKSPACE",
];

fn optional_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

use evot_engine::ThinkingLevel;

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ConfigSource {
    llm: LlmSelectionSource,
    anthropic: ProviderSource,
    openai: ProviderSource,
    server: ServerSource,
    storage: StorageSource,
    thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct LlmSelectionSource {
    provider: Option<ProviderKind>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ProviderSource {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ServerSource {
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct StorageSource {
    backend: Option<StorageBackend>,
    fs: FsStorageSource,
    cloud: CloudStorageSource,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct FsStorageSource {
    root_dir: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct CloudStorageSource {
    endpoint: Option<String>,
    api_key: Option<String>,
    workspace: Option<String>,
}

impl ConfigSource {
    fn apply(self, config: &mut Config) -> Result<()> {
        if let Some(provider) = self.llm.provider {
            config.llm.provider = provider;
        }

        if let Some(api_key) = self.anthropic.api_key {
            config.anthropic.api_key = api_key;
        }
        if let Some(base_url) = self.anthropic.base_url {
            config.anthropic.base_url = optional_string(base_url);
        }
        if let Some(model) = self.anthropic.model {
            config.anthropic.model = model;
        }

        if let Some(api_key) = self.openai.api_key {
            config.openai.api_key = api_key;
        }
        if let Some(base_url) = self.openai.base_url {
            config.openai.base_url = optional_string(base_url);
        }
        if let Some(model) = self.openai.model {
            config.openai.model = model;
        }

        if let Some(host) = self.server.host {
            config.server.host = host;
        }
        if let Some(port) = self.server.port {
            config.server.port = port;
        }

        if let Some(backend) = self.storage.backend {
            config.storage.backend = backend;
        }
        if let Some(root_dir) = self.storage.fs.root_dir {
            config.storage.fs.root_dir = paths::expand_home_path(&root_dir)?;
        }
        if let Some(endpoint) = self.storage.cloud.endpoint {
            config.storage.cloud.endpoint = endpoint;
        }
        if let Some(api_key) = self.storage.cloud.api_key {
            config.storage.cloud.api_key = api_key;
        }
        if let Some(workspace) = self.storage.cloud.workspace {
            config.storage.cloud.workspace = optional_string(workspace);
        }

        if let Some(level) = self.thinking_level {
            config.thinking_level = level;
        }

        Ok(())
    }
}

fn load_file_source(path: &Path) -> Result<ConfigSource> {
    if !path.exists() {
        return Ok(ConfigSource::default());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| EvotError::Conf(format!("failed to read {}: {e}", path.display())))?;

    let parser = toml::Deserializer::new(&content);
    serde_ignored::deserialize(parser, |unknown| {
        tracing::warn!(path = %unknown, "unknown config field");
    })
    .map_err(|e| EvotError::Conf(format!("failed to parse {}: {e}", path.display())))
}

fn load_env_file(path: &Path) -> Result<HashMap<String, String>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read(path)
        .map_err(|e| EvotError::Conf(format!("failed to read {}: {e}", path.display())))?;
    let mut vars = HashMap::new();

    for line in content.lines() {
        let line = line.map_err(|e| {
            EvotError::Conf(format!("failed to read line in {}: {e}", path.display()))
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let trimmed = match trimmed.strip_prefix("export ") {
            Some(value) => value,
            None => trimmed,
        };

        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !value.is_empty() {
                vars.insert(key, value);
            }
        }
    }

    Ok(vars)
}

fn load_process_env() -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for &key in RELEVANT_KEYS {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                vars.insert(key.to_string(), value);
            }
        }
    }
    vars
}

fn provider_keys(provider: &ProviderKind) -> (&'static str, &'static str, &'static str) {
    match provider {
        ProviderKind::Anthropic => (
            "EVOT_ANTHROPIC_API_KEY",
            "EVOT_ANTHROPIC_BASE_URL",
            "EVOT_ANTHROPIC_MODEL",
        ),
        ProviderKind::OpenAi => (
            "EVOT_OPENAI_API_KEY",
            "EVOT_OPENAI_BASE_URL",
            "EVOT_OPENAI_MODEL",
        ),
    }
}

fn apply_provider_env(config: &mut Config, provider: ProviderKind, vars: &HashMap<String, String>) {
    let provider_config = config.provider_config_mut(&provider);
    let (api_key_key, base_url_key, model_key) = provider_keys(&provider);

    if let Some(api_key) = vars.get(api_key_key) {
        provider_config.api_key = api_key.clone();
    }
    if let Some(base_url) = vars.get(base_url_key) {
        provider_config.base_url = Some(base_url.clone());
    }
    if let Some(model) = vars.get(model_key) {
        provider_config.model = model.clone();
    }
}

fn apply_env(config: &mut Config, vars: &HashMap<String, String>) -> Result<()> {
    if let Some(provider) = vars.get("EVOT_LLM_PROVIDER") {
        config.llm.provider = ProviderKind::from_str_loose(provider)?;
    }

    if let Some(level) = vars.get("EVOT_THINKING_LEVEL") {
        config.thinking_level = level.parse::<ThinkingLevel>().map_err(EvotError::Conf)?;
    }

    apply_provider_env(config, ProviderKind::Anthropic, vars);
    apply_provider_env(config, ProviderKind::OpenAi, vars);

    if let Some(host) = vars.get("EVOT_SERVER_HOST") {
        config.server.host = host.clone();
    }
    if let Some(port) = vars.get("EVOT_SERVER_PORT") {
        config.server.port = port
            .parse::<u16>()
            .map_err(|e| EvotError::Conf(format!("invalid EVOT_SERVER_PORT value {port}: {e}")))?;
    }

    if let Some(backend) = vars.get("EVOT_STORAGE_BACKEND") {
        config.storage.backend = match backend.as_str() {
            "fs" => StorageBackend::Fs,
            "cloud" => StorageBackend::Cloud,
            other => {
                return Err(EvotError::Conf(format!(
                    "unknown EVOT_STORAGE_BACKEND: {other}"
                )))
            }
        };
    }
    if let Some(root_dir) = vars.get("EVOT_STORAGE_FS_ROOT_DIR") {
        config.storage.fs.root_dir = paths::expand_home_path(root_dir)?;
    }
    if let Some(endpoint) = vars.get("EVOT_STORAGE_CLOUD_ENDPOINT") {
        config.storage.cloud.endpoint = endpoint.clone();
    }
    if let Some(api_key) = vars.get("EVOT_STORAGE_CLOUD_API_KEY") {
        config.storage.cloud.api_key = api_key.clone();
    }
    if let Some(workspace) = vars.get("EVOT_STORAGE_CLOUD_WORKSPACE") {
        config.storage.cloud.workspace = Some(workspace.clone());
    }

    Ok(())
}

pub(super) fn load_config_inner() -> Result<Config> {
    let mut config = default_config()?;

    let file_source = load_file_source(&paths::config_file_path()?)?;
    file_source.apply(&mut config)?;

    let env_file_vars = load_env_file(&paths::env_file_path()?)?;
    apply_env(&mut config, &env_file_vars)?;

    let process_vars = load_process_env();
    apply_env(&mut config, &process_vars)?;

    config.validate()?;

    Ok(config)
}
