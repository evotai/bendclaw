use bendengine::provider::model::ApiProtocol;
use bendengine::provider::registry::ProviderRegistry;

#[test]
fn default_registry_has_all_providers() {
    let registry = ProviderRegistry::default();

    assert!(registry.has(&ApiProtocol::AnthropicMessages));
    assert!(registry.has(&ApiProtocol::OpenAiCompletions));
    assert!(registry.has(&ApiProtocol::BedrockConverseStream));
}

#[test]
fn registry_protocols() {
    let registry = ProviderRegistry::default();
    let protocols = registry.protocols();
    assert_eq!(protocols.len(), 3);
}

#[test]
fn custom_registry() {
    let mut registry = ProviderRegistry::new();
    assert!(!registry.has(&ApiProtocol::AnthropicMessages));

    registry.register(
        ApiProtocol::AnthropicMessages,
        bendengine::provider::AnthropicProvider,
    );
    assert!(registry.has(&ApiProtocol::AnthropicMessages));
}
