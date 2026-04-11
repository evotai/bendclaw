use bendengine::provider::model::*;

#[test]
fn model_config_anthropic() {
    let config = ModelConfig::anthropic("claude-sonnet-4-20250514", "Claude Sonnet 4");
    assert_eq!(config.api, ApiProtocol::AnthropicMessages);
    assert_eq!(config.provider, "anthropic");
    assert!(config.compat.is_none());
}

#[test]
fn model_config_openai() {
    let config = ModelConfig::openai("gpt-4o", "GPT-4o");
    assert_eq!(config.api, ApiProtocol::OpenAiCompletions);
    let compat = config.compat.unwrap();
    assert!(compat.supports_store);
    assert!(compat.supports_developer_role);
    assert_eq!(compat.max_tokens_field, MaxTokensField::MaxCompletionTokens);
}

#[test]
fn openai_compat_variants() {
    let xai = OpenAiCompat::xai();
    assert_eq!(xai.thinking_format, ThinkingFormat::Xai);
    assert!(!xai.supports_store);

    let groq = OpenAiCompat::groq();
    assert!(groq.supports_usage_in_streaming);
    assert!(!groq.supports_store);

    let deepseek = OpenAiCompat::deepseek();
    assert_eq!(
        deepseek.max_tokens_field,
        MaxTokensField::MaxCompletionTokens
    );

    let zai = OpenAiCompat::zai();
    assert!(zai.supports_usage_in_streaming);
    assert!(!zai.supports_store);

    let minimax = OpenAiCompat::minimax();
    assert!(minimax.supports_usage_in_streaming);
    assert!(!minimax.supports_store);
}

#[test]
fn model_config_zai() {
    let config = ModelConfig::zai("glm-4.7", "GLM 4.7");
    assert_eq!(config.api, ApiProtocol::OpenAiCompletions);
    assert_eq!(config.provider, "zai");
    assert_eq!(config.base_url, "https://api.z.ai/api/paas/v4");
    assert!(config.compat.is_some());
}

#[test]
fn model_config_minimax() {
    let config = ModelConfig::minimax("MiniMax-Text-01", "MiniMax Text 01");
    assert_eq!(config.api, ApiProtocol::OpenAiCompletions);
    assert_eq!(config.provider, "minimax");
    assert_eq!(config.base_url, "https://api.minimaxi.chat/v1");
    assert_eq!(config.context_window, 1_000_000);
    assert!(config.compat.is_some());
}

#[test]
fn api_protocol_display() {
    assert_eq!(
        ApiProtocol::AnthropicMessages.to_string(),
        "anthropic_messages"
    );
    assert_eq!(
        ApiProtocol::OpenAiCompletions.to_string(),
        "openai_completions"
    );
    assert_eq!(
        ApiProtocol::GoogleGenerativeAi.to_string(),
        "google_generative_ai"
    );
}

#[test]
fn cost_config_default() {
    let cost = CostConfig::default();
    assert_eq!(cost.input_per_million, 0.0);
    assert_eq!(cost.output_per_million, 0.0);
}
