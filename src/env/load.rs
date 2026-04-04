use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use crate::env::llm::default_model;
use crate::env::llm::LlmConfig;
use crate::env::llm::ProviderKind;
use crate::env::paths;
use crate::error::BendclawError;
use crate::error::Result;

pub struct RuntimeEnv {
    pub llm: LlmConfig,
}

fn parse_env_file(path: &Path) -> Result<HashMap<String, String>> {
    let content = std::fs::read(path)
        .map_err(|e| BendclawError::Env(format!("failed to read {}: {e}", path.display())))?;
    let mut vars = HashMap::new();
    for line in content.lines() {
        let line = line.map_err(|e| {
            BendclawError::Env(format!("failed to read line in {}: {e}", path.display()))
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
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

const RELEVANT_KEYS: &[&str] = &[
    "BENDCLAW_LLM_PROVIDER",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "OPENAI_MODEL",
];

fn merge_with_process_env(file_vars: HashMap<String, String>) -> HashMap<String, String> {
    let mut merged = HashMap::new();
    for (key, value) in file_vars {
        merged.insert(key, value);
    }
    for &key in RELEVANT_KEYS {
        if let Ok(val) = std::env::var(key) {
            if !val.is_empty() {
                merged.insert(key.to_string(), val);
            }
        }
    }
    merged
}

pub fn resolve_llm_config(
    vars: &HashMap<String, String>,
    cli_model: Option<&str>,
) -> Result<LlmConfig> {
    let provider = match vars.get("BENDCLAW_LLM_PROVIDER") {
        Some(v) => ProviderKind::from_str_loose(v)?,
        None => ProviderKind::default(),
    };

    let (key_var, url_var, model_var) = match provider {
        ProviderKind::Anthropic => ("ANTHROPIC_API_KEY", "ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL"),
        ProviderKind::OpenAi => ("OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_MODEL"),
    };

    let api_key = vars
        .get(key_var)
        .cloned()
        .ok_or_else(|| BendclawError::Env(format!("{key_var} not set")))?;

    let base_url = vars.get(url_var).cloned();

    let model = cli_model
        .map(|s| s.to_string())
        .or_else(|| vars.get(model_var).cloned())
        .unwrap_or_else(|| default_model(&provider).to_string());

    Ok(LlmConfig {
        provider,
        api_key,
        base_url,
        model,
    })
}

pub fn load_runtime_env(cli_model: Option<&str>) -> Result<RuntimeEnv> {
    let file_vars = match paths::env_file_path() {
        Ok(path) if path.exists() => parse_env_file(&path)?,
        _ => HashMap::new(),
    };

    let merged = merge_with_process_env(file_vars);
    let llm = resolve_llm_config(&merged, cli_model)?;
    Ok(RuntimeEnv { llm })
}
