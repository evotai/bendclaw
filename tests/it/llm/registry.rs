use anyhow::Context as _;
use anyhow::Result;
use bendclaw::llm::config::LLMConfig;
use bendclaw::llm::config::ProviderEndpoint;
use bendclaw::llm::registry::ProviderRegistry;

fn test_endpoint(name: &str, provider: &str, weight: u32) -> ProviderEndpoint {
    ProviderEndpoint {
        name: name.into(),
        provider: provider.into(),
        base_url: "https://api.example.com".into(),
        api_key: "test-key".into(),
        model: format!("{name}-model"),
        weight,
        temperature: 0.7,
        input_price: 0.0,
        output_price: 0.0,
    }
}

// ── ProviderRegistry ──

#[test]
fn registry_with_builtins_has_openai_and_anthropic() -> Result<()> {
    let registry = ProviderRegistry::with_builtins();
    let ep = test_endpoint("test-openai", "openai", 100);
    let provider = registry.create(&ep).context("provider build")?;
    assert_eq!(provider.default_model(), "unknown");
    Ok(())
}

#[test]
fn registry_unknown_provider_returns_llm_request_error() {
    let registry = ProviderRegistry::with_builtins();
    let ep = test_endpoint("test-unknown", "deepseek", 100);
    let result = registry.create(&ep);
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert_eq!(err.code, bendclaw::types::ErrorCode::LLM_REQUEST);
}

#[test]
fn registry_custom_factory() -> Result<()> {
    let mut registry = ProviderRegistry::new();
    registry.register("custom", |_base_url: &str, _api_key: &str| {
        Ok(std::sync::Arc::new(
            crate::mocks::llm::MockLLMProvider::with_text("custom"),
        ))
    });
    let ep = test_endpoint("test-custom", "custom", 100);
    let provider = registry.create(&ep).context("provider build")?;
    assert_eq!(provider.default_model(), "unknown");
    Ok(())
}

// ── LLMConfig ──

#[test]
fn config_providers_count() {
    let config = LLMConfig {
        providers: vec![
            test_endpoint("chat1", "openai", 100),
            test_endpoint("chat2", "anthropic", 80),
        ],
        ..Default::default()
    };
    assert_eq!(config.providers.len(), 2);
}

#[test]
fn config_defaults() {
    let config = LLMConfig::default();
    assert!(config.providers.is_empty());
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.base_backoff_ms, 1000);
    assert_eq!(config.circuit_breaker_threshold, 3);
    assert_eq!(config.circuit_breaker_cooldown_secs, 60);
}
