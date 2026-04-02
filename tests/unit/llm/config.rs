use anyhow::Result;
use bendclaw::llm::config::LLMConfig;
use bendclaw::llm::config::ProviderEndpoint;

#[test]
fn test_default_config() -> Result<()> {
    let cfg = LLMConfig::default();
    assert!(cfg.providers.is_empty());
    Ok(())
}

#[test]
fn test_serde_roundtrip() -> Result<()> {
    let cfg = LLMConfig {
        providers: vec![ProviderEndpoint {
            name: "openai-gpt-4.1-mini".into(),
            provider: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: "sk-test".into(),
            model: "gpt-4.1-mini".into(),
            weight: 100,
            temperature: 0.7,
            input_price: 0.4,
            output_price: 1.6,
        }],
        ..LLMConfig::default()
    };
    let json = serde_json::to_string(&cfg)?;
    let decoded: LLMConfig = serde_json::from_str(&json)?;
    assert_eq!(decoded.providers.len(), 1);
    assert_eq!(decoded.providers[0].model, "gpt-4.1-mini");
    assert_eq!(decoded.providers[0].temperature, 0.7);
    assert_eq!(decoded.providers[0].input_price, 0.4);
    Ok(())
}

#[test]
fn test_provider_endpoint_default_weight() -> Result<()> {
    let json = r#"{"name":"x","base_url":"http://x","api_key":"","model":"m"}"#;
    let ep: ProviderEndpoint = serde_json::from_str(json)?;
    assert_eq!(ep.weight, 100);
    assert_eq!(ep.temperature, 1.0);
    Ok(())
}

#[test]
fn test_validate_rejects_empty_provider_list() {
    let err = LLMConfig::default().validate().unwrap_err();
    assert_eq!(err.code, bendclaw::types::ErrorCode::INVALID_INPUT);
    assert!(err.message.contains("providers must not be empty"));
}

#[test]
fn test_validate_rejects_duplicate_provider_names() {
    let cfg = LLMConfig {
        providers: vec![
            ProviderEndpoint {
                name: "dup".into(),
                provider: "openai".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key: "sk-1".into(),
                model: "gpt-4.1-mini".into(),
                weight: 100,
                temperature: 0.7,
                input_price: 0.0,
                output_price: 0.0,
            },
            ProviderEndpoint {
                name: "dup".into(),
                provider: "anthropic".into(),
                base_url: "https://api.anthropic.com".into(),
                api_key: "sk-2".into(),
                model: "claude-3-7-sonnet".into(),
                weight: 50,
                temperature: 0.7,
                input_price: 0.0,
                output_price: 0.0,
            },
        ],
        ..LLMConfig::default()
    };

    let err = cfg.validate().unwrap_err();
    assert_eq!(err.code, bendclaw::types::ErrorCode::INVALID_INPUT);
    assert!(err.message.contains("must be unique"));
}

#[test]
fn test_validate_accepts_known_provider_config() -> Result<()> {
    let cfg = LLMConfig {
        providers: vec![ProviderEndpoint {
            name: "primary".into(),
            provider: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: "sk-test".into(),
            model: "gpt-4.1-mini".into(),
            weight: 100,
            temperature: 0.7,
            input_price: 0.4,
            output_price: 1.6,
        }],
        ..LLMConfig::default()
    };

    cfg.validate()?;
    Ok(())
}
