use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use evot_engine::provider::CompatCaps;
use indexmap::IndexMap;

use crate::conf::default_config;
use crate::conf::infer_protocol;
use crate::conf::parse_protocol;
use crate::conf::paths;
use crate::conf::thinking_level_from_str;
use crate::conf::ChannelsConfig;
use crate::conf::Config;
use crate::conf::ProviderProfile;
use crate::conf::StorageBackend;
use crate::error::EvotError;
use crate::error::Result;
use crate::gateway::channels::feishu::FeishuChannelConfig;

// ---------------------------------------------------------------------------
// TOML source structures
// ---------------------------------------------------------------------------

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ConfigSource {
    llm: LlmSelectionSource,
    providers: IndexMap<String, ProviderSource>,
    server: ServerSource,
    storage: StorageSource,
    channel: ChannelSource,
    sandbox: SandboxSource,
    telemetry: TelemetrySource,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ChannelSource {
    feishu: Option<FeishuChannelConfig>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct LlmSelectionSource {
    provider: Option<String>,
    thinking_level: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ProviderSource {
    protocol: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    #[serde(default, deserialize_with = "deserialize_one_or_many")]
    model: Option<Vec<String>>,
    compat_caps: Option<CompatCaps>,
}

/// Deserialize a TOML value as either a single string or an array of strings.
fn deserialize_one_or_many<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
where D: serde::Deserializer<'de> {
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    let val = Option::<OneOrMany>::deserialize(deserializer)?;
    Ok(val.map(|v| match v {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    }))
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

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct SandboxSource {
    enabled: Option<bool>,
    allowed_dirs: Option<Vec<String>>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct TelemetrySource {
    endpoint: Option<String>,
    capture_content: Option<bool>,
}

fn optional_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

// ---------------------------------------------------------------------------
// TOML apply
// ---------------------------------------------------------------------------

impl ConfigSource {
    fn apply(self, config: &mut Config) -> Result<()> {
        if let Some(provider) = self.llm.provider {
            config.llm.provider = provider;
        }
        if let Some(level) = self.llm.thinking_level {
            config.llm.thinking_level = thinking_level_from_str(&level)?;
        }

        // Apply [providers.*] from TOML — preserves declaration order
        for (name, src) in self.providers {
            merge_provider_source(&mut config.providers, &name, src)?;
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

        if self.channel.feishu.is_some() {
            config.channels = ChannelsConfig {
                feishu: self.channel.feishu,
            };
        }

        if let Some(enabled) = self.sandbox.enabled {
            config.sandbox.enabled = enabled;
        }
        if let Some(dirs) = self.sandbox.allowed_dirs {
            let mut expanded = Vec::new();
            for d in dirs {
                let d = d.trim().to_string();
                if !d.is_empty() {
                    expanded.push(paths::expand_home_path(&d)?);
                }
            }
            if !expanded.is_empty() {
                config.sandbox.allowed_dirs = expanded;
            }
        }

        // Telemetry
        if let Some(endpoint) = self.telemetry.endpoint {
            config.telemetry.endpoint = Some(endpoint);
        }
        if let Some(capture_content) = self.telemetry.capture_content {
            config.telemetry.capture_content = capture_content;
        }

        Ok(())
    }
}

/// Normalize a provider name to lowercase kebab-case.
fn normalize_provider_name(name: &str) -> String {
    name.to_lowercase()
}

/// Validate that a provider name is legal (no `:` allowed).
fn validate_provider_name(name: &str) -> Result<()> {
    if name.contains(':') {
        return Err(EvotError::Conf(format!(
            "provider name '{}' must not contain ':'",
            name
        )));
    }
    Ok(())
}

/// Merge a ProviderSource into the providers IndexMap.
/// If the provider already exists, only overwrite fields that are Some.
/// If new, insert with inferred protocol.
fn merge_provider_source(
    providers: &mut IndexMap<String, ProviderProfile>,
    name: &str,
    src: ProviderSource,
) -> Result<()> {
    let name = normalize_provider_name(name);
    validate_provider_name(&name)?;
    if let Some(profile) = providers.get_mut(&name) {
        if let Some(protocol) = src.protocol {
            profile.protocol = parse_protocol(&protocol)?;
        }
        if let Some(api_key) = src.api_key {
            profile.api_key = api_key;
        }
        if let Some(base_url) = src.base_url {
            profile.base_url = base_url;
        }
        if let Some(model) = src.model {
            profile.models = model;
        }
        if let Some(compat_caps) = src.compat_caps {
            profile.compat_caps = compat_caps;
        }
    } else {
        let protocol = match src.protocol {
            Some(p) => parse_protocol(&p)?,
            None => infer_protocol(&name),
        };
        providers.insert(name, ProviderProfile {
            protocol,
            api_key: src.api_key.unwrap_or_default(),
            base_url: src.base_url.unwrap_or_default(),
            models: src.model.unwrap_or_default(),
            compat_caps: src.compat_caps.unwrap_or_default(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// TOML file loading
// ---------------------------------------------------------------------------

fn load_file_source(path: &Path) -> Result<ConfigSource> {
    if !path.exists() {
        return Ok(ConfigSource::default());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| EvotError::Conf(format!("failed to read {}: {e}", path.display())))?;
    if content.trim().is_empty() {
        return Ok(ConfigSource::default());
    }
    let source: ConfigSource = toml::from_str(&content)
        .map_err(|e| EvotError::Conf(format!("failed to parse {}: {e}", path.display())))?;
    Ok(source)
}

// ---------------------------------------------------------------------------
// Env file loading
// ---------------------------------------------------------------------------

fn load_env_file(path: &Path) -> Result<Vec<(String, String)>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path)
        .map_err(|e| EvotError::Conf(format!("failed to open {}: {e}", path.display())))?;
    let reader = std::io::BufReader::new(file);
    let mut pairs = Vec::new();
    for line in reader.lines() {
        let line =
            line.map_err(|e| EvotError::Conf(format!("failed to read {}: {e}", path.display())))?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Strip optional "export " prefix
        let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if !key.is_empty() {
                pairs.push((key, value));
            }
        }
    }
    Ok(pairs)
}

fn load_process_env() -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for (key, value) in std::env::vars() {
        if is_relevant_key(&key) {
            pairs.push((key, value));
        }
    }
    pairs
}

fn ensure_env_file(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| EvotError::Conf(format!("failed to create {}: {e}", parent.display())))?;
    }
    std::fs::write(path, default_env_content())
        .map_err(|e| EvotError::Conf(format!("failed to write {}: {e}", path.display())))?;
    Ok(())
}

fn default_env_content() -> &'static str {
    r#"# EVOT_LLM_THINKING_LEVEL=adaptive
# Anthropic: off disables thinking; all other levels use adaptive thinking.
# OpenAI-compatible: adaptive/high=high, medium=medium, minimal/low=low.

# EVOT_LLM_ANTHROPIC_API_KEY=
# EVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com
# EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-20250514
# Multiple models: EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-6,claude-opus-4-6
"#
}

// ---------------------------------------------------------------------------
// Env key classification
// ---------------------------------------------------------------------------

/// Global keys that are not provider fields.
const GLOBAL_ENV_KEYS: &[&str] = &["EVOT_LLM_PROVIDER", "EVOT_LLM_THINKING_LEVEL"];

/// Legacy key prefixes for backward compatibility.
const LEGACY_PREFIXES: &[&str] = &["EVOT_ANTHROPIC_", "EVOT_OPENAI_"];

/// Provider field suffixes.
const PROVIDER_FIELDS: &[&str] = &[
    "_API_KEY",
    "_BASE_URL",
    "_MODEL",
    "_PROTOCOL",
    "_COMPAT_CAPS",
];

/// Non-LLM keys we still care about.
const OTHER_RELEVANT_PREFIXES: &[&str] = &[
    "EVOT_SERVER_",
    "EVOT_STORAGE_",
    "EVOT_CHANNEL_",
    "EVOT_SANDBOX",
    "EVOT_SKILLS_DIRS",
    "EVOT_ID",
    "EVOT_THINKING_LEVEL",
    "EVOT_TELEMETRY_",
];

fn is_relevant_key(key: &str) -> bool {
    if key.starts_with("EVOT_LLM_") {
        return true;
    }
    for prefix in LEGACY_PREFIXES {
        if key.starts_with(prefix) {
            return true;
        }
    }
    for prefix in OTHER_RELEVANT_PREFIXES {
        if key.starts_with(prefix) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Env parsing: extract provider profiles from EVOT_LLM_{NAME}_{FIELD}
// ---------------------------------------------------------------------------

/// Convert env NAME encoding to provider name: uppercase + underscore → lowercase + hyphen.
/// e.g. "MY_CORP" → "my-corp", "OPENROUTER" → "openrouter"
fn env_name_to_provider(name: &str) -> String {
    name.to_lowercase().replace('_', "-")
}

/// Try to parse a key as EVOT_LLM_{NAME}_{FIELD}.
/// Returns (provider_name, field_suffix) if matched.
fn parse_provider_env_key(key: &str) -> Option<(String, &'static str)> {
    let rest = key.strip_prefix("EVOT_LLM_")?;

    // Skip global keys
    for gk in GLOBAL_ENV_KEYS {
        if key == *gk {
            return None;
        }
    }

    // Try each field suffix (longest first to avoid partial matches)
    for suffix in PROVIDER_FIELDS {
        if let Some(name_part) = rest.strip_suffix(suffix) {
            if !name_part.is_empty() {
                return Some((env_name_to_provider(name_part), suffix));
            }
        }
    }
    None
}

/// Parse legacy EVOT_ANTHROPIC_* / EVOT_OPENAI_* keys.
fn parse_legacy_env_key(key: &str) -> Option<(&'static str, &'static str)> {
    if let Some(field) = key.strip_prefix("EVOT_ANTHROPIC_") {
        let suffix = match field {
            "API_KEY" => "_API_KEY",
            "BASE_URL" => "_BASE_URL",
            "MODEL" => "_MODEL",
            _ => return None,
        };
        return Some(("anthropic", suffix));
    }
    if let Some(field) = key.strip_prefix("EVOT_OPENAI_") {
        let suffix = match field {
            "API_KEY" => "_API_KEY",
            "BASE_URL" => "_BASE_URL",
            "MODEL" => "_MODEL",
            _ => return None,
        };
        return Some(("openai", suffix));
    }
    None
}

fn parse_compat_caps(value: &str) -> Result<CompatCaps> {
    let mut caps = CompatCaps::NONE;
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        caps |= match part {
            "store" => CompatCaps::STORE,
            "developer_role" => CompatCaps::DEVELOPER_ROLE,
            "reasoning_effort" => CompatCaps::REASONING_EFFORT,
            "usage_in_streaming" => CompatCaps::USAGE_IN_STREAMING,
            "tool_result_name" => CompatCaps::TOOL_RESULT_NAME,
            "assistant_after_tool_result" => CompatCaps::ASSISTANT_AFTER_TOOL_RESULT,
            "reasoning_content_required" => CompatCaps::REASONING_CONTENT_REQUIRED,
            other => return Err(EvotError::Conf(format!("unknown compat cap: {other}"))),
        };
    }
    Ok(caps)
}

fn apply_provider_field(
    providers: &mut IndexMap<String, ProviderProfile>,
    name: &str,
    field: &str,
    value: &str,
) -> Result<()> {
    validate_provider_name(name)?;
    let profile = providers
        .entry(name.to_string())
        .or_insert_with(|| ProviderProfile {
            protocol: infer_protocol(name),
            api_key: String::new(),
            base_url: String::new(),
            models: Vec::new(),
            compat_caps: CompatCaps::default(),
        });
    match field {
        "_API_KEY" => profile.api_key = value.to_string(),
        "_BASE_URL" => profile.base_url = value.to_string(),
        "_MODEL" => {
            profile.models = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        "_PROTOCOL" => profile.protocol = parse_protocol(value)?,
        "_COMPAT_CAPS" => profile.compat_caps = parse_compat_caps(value)?,
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// apply_env — process ordered key-value pairs into Config
// ---------------------------------------------------------------------------

fn apply_env(config: &mut Config, vars: &[(String, String)]) -> Result<()> {
    // First pass: legacy keys (lower priority)
    for (key, value) in vars {
        if let Some((provider_name, field)) = parse_legacy_env_key(key) {
            // Only apply if no new-format key has set this provider yet
            if !has_new_format_provider(vars, provider_name) {
                apply_provider_field(&mut config.providers, provider_name, field, value)?;
            }
        }
    }

    // Second pass: new format EVOT_LLM_{NAME}_{FIELD}
    for (key, value) in vars {
        if let Some((provider_name, field)) = parse_provider_env_key(key) {
            apply_provider_field(&mut config.providers, &provider_name, field, value)?;
        }
    }

    // Global LLM keys
    for (key, value) in vars {
        match key.as_str() {
            "EVOT_LLM_PROVIDER" => config.llm.provider = value.clone(),
            "EVOT_LLM_THINKING_LEVEL" => {
                config.llm.thinking_level = thinking_level_from_str(value)?;
            }
            // Legacy thinking level key
            "EVOT_THINKING_LEVEL" => {
                config.llm.thinking_level = thinking_level_from_str(value)?;
            }
            _ => {}
        }
    }

    // Server
    for (key, value) in vars {
        match key.as_str() {
            "EVOT_SERVER_HOST" => config.server.host = value.clone(),
            "EVOT_SERVER_PORT" => {
                config.server.port = value.parse::<u16>().map_err(|e| {
                    EvotError::Conf(format!("invalid EVOT_SERVER_PORT value {value}: {e}"))
                })?;
            }
            _ => {}
        }
    }

    // Storage
    for (key, value) in vars {
        match key.as_str() {
            "EVOT_STORAGE_BACKEND" => {
                config.storage.backend = match value.as_str() {
                    "fs" => StorageBackend::Fs,
                    "cloud" => StorageBackend::Cloud,
                    other => {
                        return Err(EvotError::Conf(format!(
                            "unknown EVOT_STORAGE_BACKEND: {other}"
                        )))
                    }
                };
            }
            "EVOT_STORAGE_FS_ROOT_DIR" => {
                config.storage.fs.root_dir = paths::expand_home_path(value)?;
            }
            "EVOT_STORAGE_CLOUD_ENDPOINT" => {
                config.storage.cloud.endpoint = value.clone();
            }
            "EVOT_STORAGE_CLOUD_API_KEY" => {
                config.storage.cloud.api_key = value.clone();
            }
            "EVOT_STORAGE_CLOUD_WORKSPACE" => {
                config.storage.cloud.workspace = Some(value.clone());
            }
            _ => {}
        }
    }

    // Feishu channel
    let feishu_app_id = vars.iter().find(|(k, _)| k == "EVOT_CHANNEL_FEISHU_APP_ID");
    if let Some((_, app_id)) = feishu_app_id {
        let vars_map: HashMap<&str, &str> =
            vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let app_secret = vars_map
            .get("EVOT_CHANNEL_FEISHU_APP_SECRET")
            .copied()
            .unwrap_or_default()
            .to_string();
        let mention_only = vars_map
            .get("EVOT_CHANNEL_FEISHU_MENTION_ONLY")
            .map(|v| *v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);
        config.channels.feishu = Some(FeishuChannelConfig {
            app_id: app_id.clone(),
            app_secret,
            mention_only,
            allow_from: Vec::new(),
        });
    }

    // Sandbox
    for (key, value) in vars {
        match key.as_str() {
            "EVOT_SANDBOX" => {
                config.sandbox.enabled = value == "true" || value == "1";
            }
            "EVOT_SANDBOX_ALLOWED_DIRS" => {
                let mut dirs = Vec::new();
                for d in value.split(':') {
                    let d = d.trim();
                    if !d.is_empty() {
                        dirs.push(paths::expand_home_path(d)?);
                    }
                }
                if !dirs.is_empty() {
                    config.sandbox.allowed_dirs = dirs;
                }
            }
            _ => {}
        }
    }

    // Skills
    for (key, value) in vars {
        if key == "EVOT_SKILLS_DIRS" {
            for d in value.split(':') {
                let d = d.trim();
                if !d.is_empty() {
                    config.skills_dirs.push(paths::expand_home_path(d)?);
                }
            }
        }
    }

    // Instance ID
    for (key, value) in vars {
        if key == "EVOT_ID" {
            let val = value.trim();
            if !val.is_empty() {
                config.id = Some(val.to_string());
            }
        }
    }

    // Telemetry
    for (key, value) in vars {
        match key.as_str() {
            "EVOT_TELEMETRY_ENDPOINT" => {
                config.telemetry.endpoint = Some(value.clone());
            }
            "EVOT_TELEMETRY_CAPTURE_CONTENT" => {
                config.telemetry.capture_content = value == "true" || value == "1";
            }
            _ => {}
        }
    }

    Ok(())
}

/// Check if any new-format key (EVOT_LLM_{NAME}_*) exists for a given provider name.
fn has_new_format_provider(vars: &[(String, String)], provider_name: &str) -> bool {
    let prefix = format!(
        "EVOT_LLM_{}_",
        provider_name.to_uppercase().replace('-', "_")
    );
    vars.iter().any(|(k, _)| k.starts_with(&prefix))
}

// ---------------------------------------------------------------------------
// load_config_inner
// ---------------------------------------------------------------------------

pub(super) fn load_config_inner(env_file: Option<&str>) -> Result<Config> {
    let mut config = default_config()?;

    // 1. TOML
    let file_source = load_file_source(&paths::config_file_path()?)?;
    file_source.apply(&mut config)?;

    // 2. Env file
    let (env_path, is_custom_env) = match env_file {
        Some(path) => (paths::expand_home_path(path)?, true),
        None => (paths::default_env_file_path()?, false),
    };
    if is_custom_env {
        if !env_path.exists() {
            return Err(crate::error::EvotError::Conf(format!(
                "env file not found: {}",
                env_path.display()
            )));
        }
    } else {
        ensure_env_file(&env_path)?;
    }
    let env_file_vars = load_env_file(&env_path)?;
    apply_env(&mut config, &env_file_vars)?;

    config.env_file_path = env_path;

    // 3. Process env (highest priority)
    let process_vars = load_process_env();
    apply_env(&mut config, &process_vars)?;

    // Default provider: if not explicitly set, use the first registered provider
    if config.llm.provider.is_empty() {
        if let Some(first) = config.providers.keys().next() {
            config.llm.provider = first.clone();
        }
    }

    // Apply instance isolation: if EVOT_ID is set, redirect fs storage
    if let Some(ref id) = config.id {
        let isolated_root = paths::state_root_dir()?.join(id);
        config.storage.fs.root_dir = isolated_root;
    }

    Ok(config)
}
