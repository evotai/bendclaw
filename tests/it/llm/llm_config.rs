use bendclaw::llm::config::LLMConfig;
use bendclaw::llm::config::ProviderEndpoint;

#[test]
fn llm_config_defaults() {
    let cfg = LLMConfig::default();
    assert!(cfg.providers.is_empty());
    assert_eq!(cfg.max_retries, 3);
    assert_eq!(cfg.base_backoff_ms, 1000);
    assert_eq!(cfg.circuit_breaker_threshold, 3);
    assert_eq!(cfg.circuit_breaker_cooldown_secs, 60);
}

#[test]
fn llm_config_serde_roundtrip() {
    let cfg = LLMConfig {
        providers: vec![ProviderEndpoint {
            name: "test".into(),
            provider: "openai".into(),
            base_url: "https://api.openai.com".into(),
            api_key: "sk-test".into(),
            model: "gpt-4".into(),
            weight: 100,
            temperature: 0.5,
            input_price: 2.5,
            output_price: 10.0,
        }],
        max_retries: 5,
        base_backoff_ms: 2000,
        circuit_breaker_threshold: 5,
        circuit_breaker_cooldown_secs: 120,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: LLMConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.providers.len(), 1);
    assert_eq!(back.providers[0].name, "test");
    assert_eq!(back.max_retries, 5);
}

#[test]
fn provider_endpoint_defaults() {
    let json = r#"{
        "name": "test",
        "base_url": "http://localhost",
        "api_key": "key",
        "model": "m1"
    }"#;
    let ep: ProviderEndpoint = serde_json::from_str(json).unwrap();
    assert_eq!(ep.weight, 100);
    assert_eq!(ep.temperature, 0.7);
    assert_eq!(ep.input_price, 0.0);
    assert_eq!(ep.output_price, 0.0);
    assert!(ep.provider.is_empty());
}
