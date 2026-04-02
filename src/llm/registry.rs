use std::collections::HashMap;
use std::sync::Arc;

use super::provider::LLMProvider;
use crate::llm::config::ProviderEndpoint;
use crate::types::ErrorCode;
use crate::types::Result;

/// Factory function type for creating LLM providers.
type ProviderFactoryFn = dyn Fn(&str, &str) -> Result<Arc<dyn LLMProvider>> + Send + Sync;

/// Registry of LLM provider factories, keyed by name.
///
/// ```ignore
/// let mut registry = ProviderRegistry::with_builtins();
/// let provider = registry.create(&endpoint)?;
/// ```
pub struct ProviderRegistry {
    factories: HashMap<String, Arc<ProviderFactoryFn>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Pre-populated with OpenAI and Anthropic.
    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        r.register("openai", |base_url: &str, api_key: &str| {
            Ok(Arc::new(super::providers::openai::OpenAIProvider::new(
                base_url, api_key,
            )?) as Arc<dyn LLMProvider>)
        });
        r.register("anthropic", |base_url: &str, api_key: &str| {
            Ok(
                Arc::new(super::providers::anthropic::AnthropicProvider::new(
                    base_url, api_key,
                )?) as Arc<dyn LLMProvider>,
            )
        });
        r
    }

    /// Register a provider factory by name.
    pub fn register<F>(&mut self, name: &str, factory: F)
    where F: Fn(&str, &str) -> Result<Arc<dyn LLMProvider>> + Send + Sync + 'static {
        self.factories.insert(name.to_string(), Arc::new(factory));
    }

    /// Create a provider from an endpoint config.
    pub fn create(&self, endpoint: &ProviderEndpoint) -> Result<Arc<dyn LLMProvider>> {
        let factory = self.factories.get(&endpoint.provider).ok_or_else(|| {
            ErrorCode::llm_request(format!(
                "unknown provider '{}', available providers: {}",
                endpoint.provider,
                self.factories
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

        factory(&endpoint.base_url, &endpoint.api_key)
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}
