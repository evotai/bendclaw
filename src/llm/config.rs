use std::collections::HashSet;

use serde::Deserialize;
use serde::Serialize;

use crate::base::ErrorCode;
use crate::base::Result;

/// LLM configuration with a weighted provider list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    /// Weighted provider endpoints. Sorted by weight descending at construction.
    pub providers: Vec<ProviderEndpoint>,
    /// Max retries per LLM request before failing over. Default 3.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Base backoff in milliseconds for retry exponential backoff. Default 1000.
    #[serde(default = "default_base_backoff_ms")]
    pub base_backoff_ms: u64,
    /// Circuit breaker failure threshold before tripping. Default 3.
    #[serde(default = "default_circuit_breaker_threshold")]
    pub circuit_breaker_threshold: u32,
    /// Circuit breaker cooldown in seconds before half-open probe. Default 60.
    #[serde(default = "default_circuit_breaker_cooldown_secs")]
    pub circuit_breaker_cooldown_secs: u64,
}

/// A single LLM provider endpoint with weight for priority routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEndpoint {
    /// Unique name for this entry, used in logs and routing (e.g. "openai-gpt-4.1-mini").
    pub name: String,
    /// Service provider: "openai", "anthropic", "deepseek", etc.
    #[serde(default)]
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Higher weight = higher priority. Default 100.
    #[serde(default = "default_weight")]
    pub weight: u32,
    /// Sampling temperature for this provider. Default 0.7.
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    /// Input token price in USD per 1M tokens.
    #[serde(default)]
    pub input_price: f64,
    /// Output token price in USD per 1M tokens.
    #[serde(default)]
    pub output_price: f64,
}

fn default_weight() -> u32 {
    100
}

fn default_temperature() -> f64 {
    0.7
}

fn default_max_retries() -> u32 {
    3
}

fn default_base_backoff_ms() -> u64 {
    1000
}

fn default_circuit_breaker_threshold() -> u32 {
    3
}

fn default_circuit_breaker_cooldown_secs() -> u64 {
    60
}

impl LLMConfig {
    pub fn validate(&self) -> Result<()> {
        if self.providers.is_empty() {
            return Err(ErrorCode::invalid_input(
                "llm_config.providers must not be empty",
            ));
        }
        if self.circuit_breaker_threshold == 0 {
            return Err(ErrorCode::invalid_input(
                "llm_config.circuit_breaker_threshold must be greater than 0",
            ));
        }

        let mut names = HashSet::new();
        for (index, provider) in self.providers.iter().enumerate() {
            validate_provider_field(index, "name", &provider.name)?;
            validate_provider_field(index, "provider", &provider.provider)?;
            validate_provider_field(index, "base_url", &provider.base_url)?;
            validate_provider_field(index, "model", &provider.model)?;

            if !names.insert(provider.name.as_str()) {
                return Err(ErrorCode::invalid_input(format!(
                    "llm_config.providers[{index}].name '{}' must be unique",
                    provider.name
                )));
            }
        }

        crate::llm::router::LLMRouter::from_config(self).map_err(|error| {
            ErrorCode::invalid_input(format!("invalid llm_config: {}", error.message))
        })?;

        Ok(())
    }
}

fn validate_provider_field(index: usize, field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(ErrorCode::invalid_input(format!(
            "llm_config.providers[{index}].{field} must not be empty"
        )));
    }
    Ok(())
}

impl Default for LLMConfig {
    fn default() -> Self {
        Self {
            providers: vec![],
            max_retries: default_max_retries(),
            base_backoff_ms: default_base_backoff_ms(),
            circuit_breaker_threshold: default_circuit_breaker_threshold(),
            circuit_breaker_cooldown_secs: default_circuit_breaker_cooldown_secs(),
        }
    }
}
