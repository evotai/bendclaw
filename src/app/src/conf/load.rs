use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use evot_engine::provider::CompatCaps;
use evot_engine::provider::RouteCapabilityOverrides;
use indexmap::IndexMap;

use crate::conf::channels::FeishuChannelConfig;
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
    compat_caps: Option<ConfiguredCapabilities>,
    thinking_level: Option<String>,
    context_window: Option<u32>,
    max_tokens: Option<u32>,
    supports_image: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ConfiguredCapabilities {
    transport: CompatCaps,
    route: RouteCapabilityOverrides,
}

impl<'de> serde::Deserialize<'de> for ConfiguredCapabilities {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let names = Vec::<String>::deserialize(deserializer)?;
        parse_configured_capabilities(names.iter().map(String::as_str))
            .map_err(serde::de::Error::custom)
    }
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
            config.llm.thinking_level = Some(thinking_level_from_str(&level)?);
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
        if let Some(capabilities) = src.compat_caps {
            profile.compat_caps = capabilities.transport;
            profile.route_capabilities = capabilities.route;
        }
        if let Some(level) = src.thinking_level {
            profile.thinking_level = Some(thinking_level_from_str(&level)?);
        }
        if let Some(context_window) = src.context_window {
            profile.context_window = Some(context_window);
        }
        if let Some(max_tokens) = src.max_tokens {
            profile.max_tokens = Some(max_tokens);
        }
        if let Some(supports_image) = src.supports_image {
            profile.supports_image = Some(supports_image);
        }
    } else {
        let protocol = match src.protocol {
            Some(p) => parse_protocol(&p)?,
            None => infer_protocol(&name),
        };
        let thinking_level = match src.thinking_level {
            Some(level) => Some(thinking_level_from_str(&level)?),
            None => None,
        };
        let capabilities = src.compat_caps.unwrap_or_default();
        providers.insert(name, ProviderProfile {
            protocol,
            api_key: src.api_key.unwrap_or_default(),
            base_url: src.base_url.unwrap_or_default(),
            models: src.model.unwrap_or_default(),
            compat_caps: capabilities.transport,
            route_capabilities: capabilities.route,
            thinking_level,
            context_window: src.context_window,
            max_tokens: src.max_tokens,
            supports_image: src.supports_image,
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
    // Never replace a user file that appears between the existence check and
    // creation (for example, during concurrent startup).
    crate::atomic_file::create_private_atomic(path, default_env_content().as_bytes())?;
    Ok(())
}

fn default_env_content() -> &'static str {
    r#"# EVOT_LLM_THINKING_LEVEL=medium
# Global reasoning effort. One of off, minimal, low, medium, high, xhigh, max.
# Default: medium. Applies to every provider unless overridden per provider via
# EVOT_LLM_{PROVIDER}_THINKING_LEVEL below. Levels a model does not support are
# clamped to the nearest tier it does (searching upward first, then downward).
# Anthropic: off disables thinking; minimal/low=low, medium=medium, high=high.
#   xhigh/max=strongest efforts when the active model supports those tiers.
# OpenAI-compatible: each level maps to the matching reasoning_effort value,
#   except xhigh/max which need explicit model support (e.g. gpt-5.6).

# EVOT_LLM_ANTHROPIC_API_KEY=
# EVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com
# EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-20250514
# Multiple models: EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-6,claude-opus-4-6
# Or OpenAI Responses (must be selected explicitly)
# EVOT_LLM_OPENAI_API_KEY=
# EVOT_LLM_OPENAI_BASE_URL=https://api.openai.com/v1
# EVOT_LLM_OPENAI_MODEL=gpt-5.5
# EVOT_LLM_OPENAI_PROTOCOL=openai_responses

# Per-provider reasoning effort (overrides the global level above):
# EVOT_LLM_ANTHROPIC_THINKING_LEVEL=xhigh
# EVOT_LLM_DEEPSEEK_THINKING_LEVEL=off
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
    "_THINKING_LEVEL",
    "_CONTEXT_WINDOW",
    "_MAX_TOKENS",
    "_SUPPORTS_IMAGE",
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

fn parse_configured_capabilities<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> std::result::Result<ConfiguredCapabilities, String> {
    let mut capabilities = ConfiguredCapabilities::default();
    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if let Some(cap) = CompatCaps::from_name(name) {
            capabilities.transport |= cap;
        } else if !capabilities.route.set_named(name) {
            return Err(format!("unknown compat cap: {name}"));
        }
    }
    Ok(capabilities)
}

fn parse_compat_caps(value: &str) -> Result<ConfiguredCapabilities> {
    parse_configured_capabilities(value.split(',')).map_err(EvotError::Conf)
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
            route_capabilities: RouteCapabilityOverrides::default(),
            thinking_level: None,
            context_window: None,
            max_tokens: None,
            supports_image: None,
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
        "_COMPAT_CAPS" => {
            let capabilities = parse_compat_caps(value)?;
            profile.compat_caps = capabilities.transport;
            profile.route_capabilities = capabilities.route;
        }
        "_THINKING_LEVEL" => profile.thinking_level = Some(thinking_level_from_str(value)?),
        "_CONTEXT_WINDOW" => profile.context_window = Some(parse_token_count(name, field, value)?),
        "_MAX_TOKENS" => profile.max_tokens = Some(parse_token_count(name, field, value)?),
        "_SUPPORTS_IMAGE" => profile.supports_image = Some(parse_bool(name, field, value)?),
        _ => {}
    }
    Ok(())
}

/// Parse a positive token-count field (context window / max tokens).
fn parse_token_count(name: &str, field: &str, value: &str) -> Result<u32> {
    let parsed: u32 = value.trim().parse().map_err(|_| {
        EvotError::Conf(format!(
            "EVOT_LLM_{}{} must be a positive integer, got '{}'",
            name.to_uppercase().replace('-', "_"),
            field,
            value
        ))
    })?;
    if parsed == 0 {
        return Err(EvotError::Conf(format!(
            "EVOT_LLM_{}{} must be greater than 0",
            name.to_uppercase().replace('-', "_"),
            field
        )));
    }
    Ok(parsed)
}

/// Parse a boolean field (e.g. `_SUPPORTS_IMAGE`). Accepts common truthy/falsy
/// spellings so `true/false`, `1/0`, `yes/no`, and `on/off` all work.
fn parse_bool(name: &str, field: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(EvotError::Conf(format!(
            "EVOT_LLM_{}{} must be a boolean (true/false), got '{}'",
            name.to_uppercase().replace('-', "_"),
            field,
            value
        ))),
    }
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
                config.llm.thinking_level = Some(thinking_level_from_str(value)?);
            }
            // Legacy thinking level key
            "EVOT_THINKING_LEVEL" => {
                config.llm.thinking_level = Some(thinking_level_from_str(value)?);
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

    apply_cloud_provider(&mut config)?;
    reconcile_cloud_env(&mut config, &env_file_vars);

    if !config.providers.contains_key(&config.llm.provider) {
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

fn url_host(url: &str) -> &str {
    let rest = url
        .trim()
        .strip_prefix("https://")
        .or_else(|| url.trim().strip_prefix("http://"))
        .unwrap_or_else(|| url.trim());
    rest.split(['/', '?'])
        .next()
        .unwrap_or("")
        .trim_matches('.')
}

fn is_cloud_base_url(base_url: &str) -> bool {
    let host = url_host(base_url);
    if host.is_empty() {
        return false;
    }
    let default_host = url_host(crate::auth::DEFAULT_SERVER_URL);
    if host.eq_ignore_ascii_case(default_host) {
        return true;
    }
    std::env::var("EVOT_SERVER_URL")
        .ok()
        .is_some_and(|configured| host.eq_ignore_ascii_case(url_host(&configured)))
}

fn is_stale_cloud_profile(profile: &ProviderProfile) -> bool {
    // Endpoint alone is not ownership: a user may deliberately route a custom
    // provider through the same host. Only credentials issued by evot plus a
    // cloud endpoint identify a provider persisted by an older client.
    is_cloud_base_url(&profile.base_url) && profile.api_key.trim().starts_with("evot.")
}

fn reconcile_cloud_env(config: &mut Config, env_file_vars: &[(String, String)]) {
    let stale: Vec<String> = env_file_vars
        .iter()
        .filter_map(|(key, _)| parse_provider_env_key(key).map(|(name, _)| name))
        .filter(|name| {
            config.cloud_providers.contains(name)
                || config
                    .providers
                    .get(name)
                    .is_some_and(is_stale_cloud_profile)
        })
        .collect();
    if stale.is_empty() {
        return;
    }

    for name in &stale {
        if !config.cloud_providers.contains(name) {
            config.providers.shift_remove(name);
        }
    }

    match crate::conf::purge_providers_from_env(&config.env_file_path, &stale) {
        Ok(true) => tracing::info!(
            "removed server-managed provider keys from {}: {}",
            config.env_file_path.display(),
            stale.join(", ")
        ),
        Ok(false) => {}
        Err(error) => tracing::warn!(
            "could not clean server-managed provider keys from {}: {error}",
            config.env_file_path.display()
        ),
    }
}

/// Register the cloud providers from the models cache when the user is logged
/// in. Cached-only: no network at config load time; `evot login` refreshes the
/// cache. Never overrides an explicit BYOK selection.
///
/// The server names, orders, and groups its own providers (one per tier and
/// protocol), so one account can mix Anthropic and OpenAI models. Which model
/// a fresh session lands on is decided by catalog rank in
/// [`Config::preferred_new_session_llm`], not here.
fn apply_cloud_provider(config: &mut Config) -> Result<()> {
    if crate::auth::load_auth()?.is_none() {
        return Ok(());
    }
    let Some(cache) = crate::auth::load_models_cache()? else {
        return Ok(());
    };

    let mut thinking_levels = std::collections::HashMap::new();
    let mut model_tiers = std::collections::HashMap::new();
    let mut model_sorts = std::collections::HashMap::new();
    for model in &cache.response.models {
        if let Ok(level) = thinking_level_from_str(&model.thinking_level) {
            thinking_levels.insert(model.id.clone(), level);
        }
        if !model.tier.is_empty() {
            model_tiers.insert(model.id.clone(), model.tier.clone());
        }
        model_sorts.insert(model.id.clone(), model.sort_order);
    }
    // Providers are stored in server rank order, so ties in catalog rank fall
    // back to the order the server wants its groups shown in.
    let mut groups = cache.response.providers;
    groups.sort_by_key(|group| group.sort_order);
    for group in groups {
        if group.models.is_empty() {
            continue;
        }
        let name = normalize_provider_name(&group.name);
        validate_provider_name(&name)?;
        let protocol = parse_protocol(&group.protocol).map_err(|_| {
            EvotError::Conf(format!("unsupported cloud protocol: {}", group.protocol))
        })?;

        // A catalog routing name is not ownership. If the user already has a
        // custom provider with that name, keep it; only replace a profile that
        // is identifiable as cloud state persisted by an older client.
        let custom_collision = config
            .providers
            .get(&name)
            .is_some_and(|profile| !is_stale_cloud_profile(profile));
        if custom_collision {
            tracing::warn!(provider = %name, "cloud provider name collides with custom provider; keeping custom config");
            continue;
        }

        let profile = ProviderProfile {
            protocol,
            api_key: group.api_key,
            base_url: group.base_url,
            models: group.models,
            compat_caps: CompatCaps::default(),
            route_capabilities: RouteCapabilityOverrides::default(),
            thinking_level: None,
            context_window: None,
            max_tokens: None,
            supports_image: None,
        };
        config.providers.insert(name.clone(), profile);
        config.cloud_providers.insert(name);
    }
    config.cloud_thinking_levels = thinking_levels;
    config.cloud_model_tiers = model_tiers;
    config.cloud_model_sorts = model_sorts;

    // The catalog owns the landing spot, so a stale cloud selection (e.g. a
    // Free provider left in the env file) yields to it. BYOK always wins: a
    // configured provider with its own key keeps serving.
    let byok_active = !config.cloud_providers.contains(&config.llm.provider)
        && config
            .providers
            .get(&config.llm.provider)
            .is_some_and(|profile| !profile.api_key.trim().is_empty());
    if byok_active {
        return Ok(());
    }
    if let Some((provider, _)) = config.preferred_new_session_llm() {
        config.llm.provider = provider;
        config.llm.model_override = None;
    }
    Ok(())
}
