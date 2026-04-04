use std::collections::HashMap;

use bendclaw::env::resolve_llm_config;
use bendclaw::env::ProviderKind;

#[test]
fn default_provider_is_anthropic() {
    assert_eq!(ProviderKind::default(), ProviderKind::Anthropic);
}

#[test]
fn provider_kind_from_str() {
    assert_eq!(
        ProviderKind::from_str_loose("anthropic").unwrap(),
        ProviderKind::Anthropic,
    );
    assert_eq!(
        ProviderKind::from_str_loose("openai").unwrap(),
        ProviderKind::OpenAi,
    );
    assert_eq!(
        ProviderKind::from_str_loose("ANTHROPIC").unwrap(),
        ProviderKind::Anthropic,
    );
    assert!(ProviderKind::from_str_loose("unknown").is_err());
}

#[test]
fn resolve_config_from_file_vars() {
    let mut vars = HashMap::new();
    vars.insert("ANTHROPIC_API_KEY".into(), "file-key".into());
    vars.insert("ANTHROPIC_MODEL".into(), "file-model".into());

    let config = resolve_llm_config(&vars, None).unwrap();
    assert_eq!(config.provider, ProviderKind::Anthropic);
    assert_eq!(config.api_key, "file-key");
    assert_eq!(config.model, "file-model");
}

#[test]
fn resolve_config_cli_model_overrides_file() {
    let mut vars = HashMap::new();
    vars.insert("ANTHROPIC_API_KEY".into(), "file-key".into());
    vars.insert("ANTHROPIC_MODEL".into(), "file-model".into());

    let config = resolve_llm_config(&vars, Some("cli-model")).unwrap();
    assert_eq!(config.model, "cli-model");
}

#[test]
fn resolve_config_missing_key_returns_error() {
    let vars = HashMap::new();
    let result = resolve_llm_config(&vars, None);
    assert!(result.is_err());
}

#[test]
fn resolve_config_openai_provider() {
    let mut vars = HashMap::new();
    vars.insert("BENDCLAW_LLM_PROVIDER".into(), "openai".into());
    vars.insert("OPENAI_API_KEY".into(), "oai-key".into());

    let config = resolve_llm_config(&vars, None).unwrap();
    assert_eq!(config.provider, ProviderKind::OpenAi);
    assert_eq!(config.api_key, "oai-key");
    assert_eq!(config.model, "gpt-4o");
}

#[test]
fn resolve_config_default_model_per_provider() {
    let mut vars = HashMap::new();
    vars.insert("ANTHROPIC_API_KEY".into(), "key".into());

    let config = resolve_llm_config(&vars, None).unwrap();
    assert_eq!(config.model, "claude-sonnet-4-20250514");
}
