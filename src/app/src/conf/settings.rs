use std::path::Path;

use evot_engine::provider::CompatCaps;
use indexmap::IndexMap;

use crate::conf::channels::FeishuChannelConfig;
use crate::conf::env_writer::EnvGroup;
use crate::conf::parse_protocol;
use crate::conf::thinking_level_from_str;
use crate::conf::Config;
use crate::conf::Protocol;
use crate::conf::ProviderProfile;
use crate::error::EvotError;
use crate::error::Result;

/// Encode a provider name for use in an env key segment: lowercase + hyphen
/// becomes uppercase + underscore. Inverse of `env_name_to_provider` in
/// `load.rs`. e.g. `"my-corp"` -> `"MY_CORP"`.
pub fn provider_to_env_name(name: &str) -> String {
    name.to_uppercase().replace('-', "_")
}

/// One provider as edited from the settings UI.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProviderSettings {
    pub name: String,
    pub protocol: String,
    /// New API key. `None` or empty means "leave the persisted value unchanged".
    #[serde(default)]
    pub api_key: Option<String>,
    pub base_url: String,
    /// Ordered model list; the first entry is the default.
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub thinking_level: Option<String>,
}

/// Feishu fields as edited from the settings UI.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FeishuSettings {
    pub app_id: String,
    /// New app secret. `None` or empty means "leave the persisted value unchanged".
    #[serde(default)]
    pub app_secret: Option<String>,
    #[serde(default = "default_true")]
    pub mention_only: bool,
}

fn default_true() -> bool {
    true
}

/// Full settings payload accepted by `POST /api/settings`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SettingsUpdate {
    pub active_provider: String,
    #[serde(default)]
    pub thinking_level: Option<String>,
    pub providers: Vec<ProviderSettings>,
    #[serde(default)]
    pub feishu: Option<FeishuSettings>,
}

/// Build the titled env groups to persist from the resolved `Config`. This is
/// the single source of truth: the managed block contains the complete
/// configuration (including secrets), so nothing is ever left stranded outside
/// it. Call after [`apply_settings`] has merged the update into the config.
///
/// Each provider gets its own group; global LLM selection and Feishu are
/// separate groups. The returned groups feed
/// [`crate::conf::env_writer::write_grouped`], which renders them as one
/// deduplicated, organized managed block.
pub fn config_to_env_groups(config: &Config) -> Vec<EnvGroup> {
    let mut groups: Vec<EnvGroup> = Vec::new();

    let mut global = EnvGroup::new("Active selection");
    global.push("EVOT_LLM_PROVIDER", config.llm.provider.clone());
    if let Some(level) = config.llm.thinking_level {
        global.push("EVOT_LLM_THINKING_LEVEL", level.as_str());
    }
    groups.push(global);

    for (name, p) in &config.providers {
        if config.cloud_providers.contains(name) {
            continue;
        }
        let seg = provider_to_env_name(name);
        let mut g = EnvGroup::new(format!("Provider: {name}"));
        g.push(format!("EVOT_LLM_{seg}_PROTOCOL"), p.protocol.to_string());
        g.push(format!("EVOT_LLM_{seg}_BASE_URL"), p.base_url.clone());
        g.push(format!("EVOT_LLM_{seg}_MODEL"), p.models.join(","));
        if !p.api_key.is_empty() {
            g.push(format!("EVOT_LLM_{seg}_API_KEY"), p.api_key.clone());
        }
        if let Some(level) = p.thinking_level {
            g.push(format!("EVOT_LLM_{seg}_THINKING_LEVEL"), level.as_str());
        }
        if let Some(context_window) = p.context_window {
            g.push(
                format!("EVOT_LLM_{seg}_CONTEXT_WINDOW"),
                context_window.to_string(),
            );
        }
        if let Some(max_tokens) = p.max_tokens {
            g.push(format!("EVOT_LLM_{seg}_MAX_TOKENS"), max_tokens.to_string());
        }
        if let Some(supports_image) = p.supports_image {
            g.push(
                format!("EVOT_LLM_{seg}_SUPPORTS_IMAGE"),
                if supports_image { "true" } else { "false" },
            );
        }
        groups.push(g);
    }

    if let Some(f) = &config.channels.feishu {
        let mut g = EnvGroup::new("Channel: Feishu bot");
        g.push("EVOT_CHANNEL_FEISHU_APP_ID", f.app_id.clone());
        if !f.app_secret.is_empty() {
            g.push("EVOT_CHANNEL_FEISHU_APP_SECRET", f.app_secret.clone());
        }
        g.push(
            "EVOT_CHANNEL_FEISHU_MENTION_ONLY",
            if f.mention_only { "true" } else { "false" },
        );
        groups.push(g);
    }

    groups
}

/// Persist an interactively chosen thinking level as the default for future
/// sessions: updates the global selection and, when the active provider
/// carries its own override (which would otherwise mask the global), that
/// override too, then rewrites the managed env block.
pub fn persist_default_thinking_level(
    config: &mut Config,
    provider: &str,
    level: evot_engine::ThinkingLevel,
) -> Result<()> {
    config.llm.thinking_level = Some(level);
    if let Some(profile) = config.providers.get_mut(provider) {
        if profile.thinking_level.is_some() {
            profile.thinking_level = Some(level);
        }
    }
    let groups = config_to_env_groups(config);
    crate::conf::env_writer::write_grouped(&config.env_file_path, &groups)
}

/// Validate and apply a settings update to a live `Config` in place.
///
/// Secrets that arrive empty are preserved from the existing config rather than
/// cleared. Provider names, protocols, and thinking levels are validated; an
/// invalid value aborts the whole apply so the config is never left partial.
/// On success the active provider's `LlmConfig` should be rebuilt by the caller
/// via [`Config::active_llm`].
pub fn apply_settings(config: &mut Config, update: &SettingsUpdate) -> Result<()> {
    let mut providers: IndexMap<String, ProviderProfile> = IndexMap::new();
    for p in &update.providers {
        let name = p.name.trim().to_lowercase();
        if name.is_empty() {
            return Err(EvotError::Conf("provider name must not be empty".into()));
        }
        if name.contains(':') {
            return Err(EvotError::Conf(format!(
                "provider name '{name}' must not contain ':'"
            )));
        }
        if config.cloud_providers.contains(&name) {
            continue;
        }
        let protocol = parse_protocol(&p.protocol)?;
        let thinking_level = match p.thinking_level.as_deref().filter(|s| !s.is_empty()) {
            Some(level) => Some(thinking_level_from_str(level)?),
            None => None,
        };
        // Preserve existing values for the same provider so blank secrets and
        // compat caps survive a round-trip through the UI.
        let existing = config.providers.get(&name);
        let api_key = match p.api_key.as_deref().filter(|s| !s.is_empty()) {
            Some(key) => key.to_string(),
            None => existing.map(|e| e.api_key.clone()).unwrap_or_default(),
        };
        let compat_caps = existing.map(|e| e.compat_caps).unwrap_or(CompatCaps::NONE);
        let route_capabilities = existing.map(|e| e.route_capabilities).unwrap_or_default();
        let context_window = existing.and_then(|e| e.context_window);
        let max_tokens = existing.and_then(|e| e.max_tokens);
        let supports_image = existing.and_then(|e| e.supports_image);
        let models: Vec<String> = p
            .models
            .iter()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();
        providers.insert(name, ProviderProfile {
            protocol,
            api_key,
            base_url: p.base_url.trim().to_string(),
            models,
            compat_caps,
            route_capabilities,
            thinking_level,
            context_window,
            max_tokens,
            supports_image,
        });
    }

    for (name, profile) in &config.providers {
        if config.cloud_providers.contains(name) {
            providers.insert(name.clone(), profile.clone());
        }
    }

    let active = update.active_provider.trim().to_lowercase();
    if !providers.contains_key(&active) {
        return Err(EvotError::Conf(format!(
            "active provider '{active}' is not in the provider list"
        )));
    }

    config.llm.thinking_level = update
        .thinking_level
        .as_deref()
        .filter(|level| !level.is_empty())
        .map(thinking_level_from_str)
        .transpose()?;

    if let Some(f) = &update.feishu {
        let app_id = f.app_id.trim().to_string();
        if app_id.is_empty() {
            config.channels.feishu = None;
        } else {
            let existing_secret = config
                .channels
                .feishu
                .as_ref()
                .map(|e| e.app_secret.clone())
                .unwrap_or_default();
            let app_secret = match f.app_secret.as_deref().filter(|s| !s.is_empty()) {
                Some(secret) => secret.to_string(),
                None => existing_secret,
            };
            let allow_from = config
                .channels
                .feishu
                .as_ref()
                .map(|e| e.allow_from.clone())
                .unwrap_or_default();
            config.channels.feishu = Some(FeishuChannelConfig {
                app_id,
                app_secret,
                mention_only: f.mention_only,
                allow_from,
            });
        }
    }

    config.providers = providers;
    config.llm.provider = active;
    config.llm.model_override = None;
    Ok(())
}

pub fn purge_providers_from_env(env_path: &Path, providers: &[String]) -> Result<bool> {
    if providers.is_empty() {
        return Ok(false);
    }
    let prefixes: Vec<String> = providers
        .iter()
        .map(|name| format!("EVOT_LLM_{}_", provider_to_env_name(name)))
        .collect();
    crate::conf::env_writer::remove_keys(env_path, |key| {
        prefixes
            .iter()
            .any(|prefix| key.starts_with(prefix.as_str()))
    })
}

/// Mask a secret for display: show only the last 4 characters, or an empty
/// string when unset. Never returns the raw value.
fn mask_secret(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let tail: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("****{tail}")
}

/// Serialize the current config into a UI-friendly JSON snapshot with secrets
/// masked. Used by `GET /api/settings`.
pub fn settings_snapshot(config: &Config) -> serde_json::Value {
    let providers: Vec<serde_json::Value> = config
        .providers
        .iter()
        .map(|(name, p)| {
            serde_json::json!({
                "name": name,
                "cloud": config.cloud_providers.contains(name),
                "protocol": p.protocol.to_string(),
                "api_key_set": !p.api_key.trim().is_empty(),
                "api_key_hint": mask_secret(&p.api_key),
                "base_url": p.base_url,
                "models": p.models,
                "thinking_level": p.thinking_level.map(|l| l.as_str()),
            })
        })
        .collect();

    let feishu = config
        .channels
        .feishu
        .as_ref()
        .map(|f: &FeishuChannelConfig| {
            serde_json::json!({
                "app_id": f.app_id,
                "app_secret_set": !f.app_secret.trim().is_empty(),
                "app_secret_hint": mask_secret(&f.app_secret),
                "mention_only": f.mention_only,
            })
        });

    serde_json::json!({
        "active_provider": config.llm.provider,
        "thinking_level": config.llm.thinking_level.map(|level| level.as_str()),
        "protocols": [
            Protocol::Anthropic.to_string(),
            Protocol::OpenAi.to_string(),
            Protocol::OpenAiResponses.to_string(),
        ],
        "thinking_levels": ["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        "providers": providers,
        "feishu": feishu,
        "env_file_path": config.env_file_path.display().to_string(),
    })
}
