use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::agent_config::CheckpointConfig;

// ── AgentConfig defaults ──

#[test]
fn agent_config_defaults() {
    let config = AgentConfig::default();
    assert!(config.databend_api_base_url.is_empty());
    assert!(config.databend_api_token.is_empty());
    assert_eq!(config.databend_warehouse, "default");
    assert_eq!(config.skills_dir, "./skills");
    assert_eq!(config.max_iterations, 20);
    assert_eq!(config.max_context_tokens, 250_000);
    assert_eq!(config.max_duration_secs, 300);
}

#[test]
fn agent_config_serde() -> Result<(), Box<dyn std::error::Error>> {
    let config = AgentConfig::default();
    let json = serde_json::to_string(&config)?;
    assert!(json.contains("max_iterations"));
    assert!(json.contains("max_context_tokens"));
    assert!(json.contains("max_duration_secs"));
    Ok(())
}

// ── CheckpointConfig defaults ──

#[test]
fn checkpoint_config_defaults() {
    let config = CheckpointConfig::default();
    assert!(config.enabled);
    assert_eq!(config.threshold, 5);
    assert!(!config.prompt.is_empty());
    assert!(config.prompt.contains("Checkpoint"));
}

#[test]
fn checkpoint_config_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let config = CheckpointConfig {
        enabled: false,
        threshold: 10,
        prompt: "custom prompt".into(),
    };
    let json = serde_json::to_string(&config)?;
    let parsed: CheckpointConfig = serde_json::from_str(&json)?;
    assert!(!parsed.enabled);
    assert_eq!(parsed.threshold, 10);
    assert_eq!(parsed.prompt, "custom prompt");
    Ok(())
}

#[test]
fn checkpoint_config_deserialize_with_defaults() -> Result<(), Box<dyn std::error::Error>> {
    let json = "{}";
    let config: CheckpointConfig = serde_json::from_str(json)?;
    assert!(config.enabled);
    assert_eq!(config.threshold, 5);
    assert!(config.prompt.contains("Checkpoint"));
    Ok(())
}

#[test]
fn checkpoint_config_partial_deserialize() -> Result<(), Box<dyn std::error::Error>> {
    let json = r#"{"enabled": false}"#;
    let config: CheckpointConfig = serde_json::from_str(json)?;
    assert!(!config.enabled);
    assert_eq!(config.threshold, 5);
    Ok(())
}
