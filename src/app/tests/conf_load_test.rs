use std::ffi::OsString;
use std::sync::Mutex;
use std::sync::OnceLock;

use evot::conf::thinking_level_from_str;
use evot::conf::Config;
use evot::conf::Protocol;
use evot::conf::StorageBackend;
use evot_engine::ThinkingLevel;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn restore_env_var(key: &str, value: Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[test]
fn default_provider_is_anthropic() {
    let config = Config::new(std::path::PathBuf::from("/tmp"));
    assert_eq!(config.llm.provider, "anthropic");
}

#[test]
fn protocol_infer() {
    use evot::conf::infer_protocol;
    assert_eq!(infer_protocol("anthropic"), Protocol::Anthropic);
    assert_eq!(infer_protocol("openai"), Protocol::OpenAi);
    assert_eq!(infer_protocol("openrouter"), Protocol::OpenAi);
    assert_eq!(infer_protocol("deepseek"), Protocol::OpenAi);
}

#[test]
fn parse_protocol_valid() -> TestResult {
    use evot::conf::parse_protocol;
    assert_eq!(parse_protocol("anthropic")?, Protocol::Anthropic);
    assert_eq!(parse_protocol("openai")?, Protocol::OpenAi);
    assert!(parse_protocol("unknown").is_err());
    Ok(())
}

#[test]
fn config_with_port_overrides_server_port() {
    let config = Config::new(std::path::PathBuf::from("/tmp")).with_port(9999);
    assert_eq!(config.server.port, 9999);
}

#[test]
fn load_config_prefers_process_env_over_env_file() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.env"),
        "EVOT_LLM_ANTHROPIC_API_KEY=file-key\nEVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com\nEVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-20250514\nEVOT_SERVER_PORT=9010\n",
    )?;

    let original_home = std::env::var_os("HOME");
    let original_key = std::env::var_os("EVOT_LLM_ANTHROPIC_API_KEY");
    let original_port = std::env::var_os("EVOT_SERVER_PORT");

    std::env::set_var("HOME", &env_home);
    std::env::set_var("EVOT_LLM_ANTHROPIC_API_KEY", "process-key");
    std::env::set_var("EVOT_SERVER_PORT", "9020");

    let result = Config::load();

    restore_env_var("HOME", original_home);
    restore_env_var("EVOT_LLM_ANTHROPIC_API_KEY", original_key);
    restore_env_var("EVOT_SERVER_PORT", original_port);

    let config = result?;
    assert_eq!(config.active_llm()?.api_key, "process-key");
    assert_eq!(config.server.port, 9020);
    assert_eq!(config.storage.backend, StorageBackend::Fs);
    assert_eq!(config.storage.fs.root_dir, env_home.join(".evotai"));
    Ok(())
}

#[test]
fn load_config_uses_toml_then_env_then_cli() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.toml"),
        r#"
[llm]
provider = "anthropic"

[providers.anthropic]
api_key = "toml-key"
base_url = "https://api.anthropic.com"
model = "toml-model"

[server]
host = "0.0.0.0"
port = 8010

[storage]
backend = "fs"

[storage.fs]
root_dir = "~/custom-store"
"#,
    )?;
    std::fs::write(
        env_home.join(".evotai").join("evot.env"),
        "export EVOT_LLM_ANTHROPIC_MODEL=env-model\nexport EVOT_SERVER_PORT=9010\n",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load().and_then(|c| c.with_model(Some("anthropic:cli-model".into())));

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.active_llm()?.api_key, "toml-key");
    assert_eq!(config.active_llm()?.model, "cli-model");
    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 9010);
    assert_eq!(config.storage.fs.root_dir, env_home.join("custom-store"));
    Ok(())
}

#[test]
fn load_config_keeps_multiple_providers() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.toml"),
        r#"
[llm]
provider = "anthropic"

[providers.anthropic]
api_key = "anthropic-key"
base_url = "https://api.anthropic.com"
model = "claude-sonnet-4-20250514"

[providers.openai]
api_key = "openai-key"
base_url = "https://api.openai.com/v1"
model = "gpt-5"
"#,
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.active_llm()?.provider, "anthropic");
    assert_eq!(config.active_llm()?.api_key, "anthropic-key");

    // Switch provider
    let config = config.with_model(Some("gpt-5".into()))?;
    assert_eq!(config.active_llm()?.api_key, "openai-key");
    assert_eq!(config.active_llm()?.model, "gpt-5");
    Ok(())
}

#[test]
fn load_config_new_env_format() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.env"),
        "\
EVOT_LLM_PROVIDER=deepseek
EVOT_LLM_ANTHROPIC_API_KEY=ant-key
EVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com
EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-20250514
EVOT_LLM_DEEPSEEK_API_KEY=ds-key
EVOT_LLM_DEEPSEEK_BASE_URL=https://api.deepseek.com
EVOT_LLM_DEEPSEEK_MODEL=deepseek-chat
",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.llm.provider, "deepseek");
    assert_eq!(config.active_llm()?.api_key, "ds-key");
    assert_eq!(config.active_llm()?.base_url, "https://api.deepseek.com");
    assert_eq!(config.active_llm()?.model, "deepseek-chat");
    assert_eq!(config.active_llm()?.protocol, Protocol::OpenAi);

    // anthropic provider also loaded
    let ant = config.providers.get("anthropic").unwrap();
    assert_eq!(ant.api_key, "ant-key");
    assert_eq!(ant.protocol, Protocol::Anthropic);
    Ok(())
}

#[test]
fn load_config_legacy_env_compat() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.env"),
        "\
EVOT_LLM_PROVIDER=anthropic
EVOT_ANTHROPIC_API_KEY=legacy-key
EVOT_ANTHROPIC_BASE_URL=https://api.anthropic.com
EVOT_ANTHROPIC_MODEL=claude-sonnet-4-20250514
",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.active_llm()?.api_key, "legacy-key");
    assert_eq!(config.active_llm()?.protocol, Protocol::Anthropic);
    Ok(())
}

#[test]
fn thinking_level_from_str_valid() -> TestResult {
    assert_eq!(thinking_level_from_str("off")?, ThinkingLevel::Off);
    assert_eq!(thinking_level_from_str("minimal")?, ThinkingLevel::Minimal);
    assert_eq!(thinking_level_from_str("low")?, ThinkingLevel::Low);
    assert_eq!(thinking_level_from_str("medium")?, ThinkingLevel::Medium);
    assert_eq!(thinking_level_from_str("high")?, ThinkingLevel::High);
    // case-insensitive
    assert_eq!(thinking_level_from_str("HIGH")?, ThinkingLevel::High);
    assert_eq!(thinking_level_from_str("Medium")?, ThinkingLevel::Medium);
    Ok(())
}

#[test]
fn thinking_level_from_str_rejects_invalid() {
    assert!(thinking_level_from_str("turbo").is_err());
    assert!(thinking_level_from_str("").is_err());
}

#[test]
fn default_thinking_level_is_off() {
    let config = Config::new(std::path::PathBuf::from("/tmp"));
    assert_eq!(config.llm.thinking_level, ThinkingLevel::Off);
}

#[test]
fn load_config_thinking_level_from_toml() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.toml"),
        r#"
[llm]
provider = "anthropic"
thinking_level = "medium"

[providers.anthropic]
api_key = "test-key"
base_url = "https://api.anthropic.com"
model = "claude-sonnet-4-20250514"
"#,
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.llm.thinking_level, ThinkingLevel::Medium);
    assert_eq!(config.active_llm()?.thinking_level, ThinkingLevel::Medium);
    Ok(())
}

#[test]
fn load_config_thinking_level_from_env_file() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.env"),
        "EVOT_LLM_ANTHROPIC_API_KEY=test-key\nEVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com\nEVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-20250514\nEVOT_LLM_THINKING_LEVEL=high\n",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.llm.thinking_level, ThinkingLevel::High);
    Ok(())
}

#[test]
fn load_config_thinking_level_env_overrides_toml() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.toml"),
        r#"
[llm]
provider = "anthropic"
thinking_level = "low"

[providers.anthropic]
api_key = "test-key"
base_url = "https://api.anthropic.com"
model = "claude-sonnet-4-20250514"
"#,
    )?;
    std::fs::write(
        env_home.join(".evotai").join("evot.env"),
        "EVOT_LLM_THINKING_LEVEL=high\n",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.llm.thinking_level, ThinkingLevel::High);
    Ok(())
}

#[test]
fn load_config_custom_env_file_via_explicit_path() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;

    // Write a custom env file outside the default location
    let custom_env = temp.path().join("custom.env");
    std::fs::write(
        &custom_env,
        "EVOT_LLM_ANTHROPIC_API_KEY=custom-key\nEVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com\nEVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-20250514\nEVOT_SERVER_PORT=7777\n",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load_with_env_file(Some(custom_env.to_str().unwrap()));

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.active_llm()?.api_key, "custom-key");
    assert_eq!(config.server.port, 7777);
    Ok(())
}

#[test]
fn load_config_custom_env_file_missing_returns_error() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load_with_env_file(Some("/tmp/nonexistent-evot.env"));

    restore_env_var("HOME", original_home);

    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("env file not found"));
    Ok(())
}

#[test]
fn resolve_model_spec_by_model_name() -> TestResult {
    let mut config = Config::new(std::path::PathBuf::from("/tmp"));
    config
        .providers
        .insert("anthropic".into(), evot::conf::ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "key".into(),
            base_url: "https://api.anthropic.com".into(),
            model: "claude-sonnet-4-20250514".into(),
        });
    config
        .providers
        .insert("deepseek".into(), evot::conf::ProviderProfile {
            protocol: Protocol::OpenAi,
            api_key: "ds-key".into(),
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-chat".into(),
        });

    let (name, override_model) = config.resolve_model_spec("deepseek-chat")?;
    assert_eq!(name, "deepseek");
    assert_eq!(override_model, None);

    let (name, override_model) = config.resolve_model_spec("anthropic:custom-model")?;
    assert_eq!(name, "anthropic");
    assert_eq!(override_model, Some("custom-model".to_string()));

    assert!(config.resolve_model_spec("nonexistent-model").is_err());
    assert!(config.resolve_model_spec("badprovider:model").is_err());
    // Empty model in provider:model spec should be rejected
    let err = config.resolve_model_spec("anthropic:").unwrap_err();
    assert!(format!("{err}").contains("empty model"));
    Ok(())
}

#[test]
fn with_model_sets_override() -> TestResult {
    let mut config = Config::new(std::path::PathBuf::from("/tmp"));
    config
        .providers
        .insert("anthropic".into(), evot::conf::ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "key".into(),
            base_url: "https://api.anthropic.com".into(),
            model: "claude-sonnet-4-20250514".into(),
        });

    // provider:model sets override
    let config = config.with_model(Some("anthropic:custom-model".into()))?;
    assert_eq!(config.llm.provider, "anthropic");
    assert_eq!(config.llm.model_override, Some("custom-model".to_string()));
    assert_eq!(config.active_llm()?.model, "custom-model");

    // plain model match clears override
    let config = config.with_model(Some("claude-sonnet-4-20250514".into()))?;
    assert_eq!(config.llm.provider, "anthropic");
    assert_eq!(config.llm.model_override, None);
    assert_eq!(config.active_llm()?.model, "claude-sonnet-4-20250514");
    Ok(())
}

#[test]
fn with_model_none_is_noop() -> TestResult {
    let mut config = Config::new(std::path::PathBuf::from("/tmp"));
    config
        .providers
        .insert("anthropic".into(), evot::conf::ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "key".into(),
            base_url: "https://api.anthropic.com".into(),
            model: "claude-sonnet-4-20250514".into(),
        });
    let config = config.with_model(None)?;
    assert_eq!(config.llm.provider, "anthropic");
    assert_eq!(config.llm.model_override, None);
    Ok(())
}

#[test]
fn validate_missing_provider() {
    let config = Config::new(std::path::PathBuf::from("/tmp"));
    // No providers configured, default provider "anthropic" won't be found
    let err = config.validate().unwrap_err();
    assert!(format!("{err}").contains("not found"));
}

#[test]
fn validate_missing_api_key() {
    let mut config = Config::new(std::path::PathBuf::from("/tmp"));
    config
        .providers
        .insert("anthropic".into(), evot::conf::ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: String::new(),
            base_url: "https://api.anthropic.com".into(),
            model: "claude-sonnet-4-20250514".into(),
        });
    let err = config.validate().unwrap_err();
    assert!(format!("{err}").contains("api_key not set"));
}

#[test]
fn validate_missing_base_url() {
    let mut config = Config::new(std::path::PathBuf::from("/tmp"));
    config
        .providers
        .insert("anthropic".into(), evot::conf::ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "key".into(),
            base_url: String::new(),
            model: "claude-sonnet-4-20250514".into(),
        });
    let err = config.validate().unwrap_err();
    assert!(format!("{err}").contains("base_url not set"));
}

#[test]
fn validate_missing_model() {
    let mut config = Config::new(std::path::PathBuf::from("/tmp"));
    config
        .providers
        .insert("anthropic".into(), evot::conf::ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "key".into(),
            base_url: "https://api.anthropic.com".into(),
            model: String::new(),
        });
    let err = config.validate().unwrap_err();
    assert!(format!("{err}").contains("model not set"));
}

#[test]
fn validate_model_override_bypasses_empty_profile_model() {
    let mut config = Config::new(std::path::PathBuf::from("/tmp"));
    config
        .providers
        .insert("anthropic".into(), evot::conf::ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "key".into(),
            base_url: "https://api.anthropic.com".into(),
            model: String::new(),
        });
    config.llm.model_override = Some("override-model".into());
    // Should pass because model_override is set
    assert!(config.validate().is_ok());
}

#[test]
fn provider_name_with_colon_rejected_in_toml() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.toml"),
        r#"
[llm]
provider = "anthropic"

[providers.anthropic]
api_key = "key"
base_url = "https://api.anthropic.com"
model = "claude-sonnet-4-20250514"

[providers."bad:name"]
api_key = "key"
base_url = "https://example.com"
model = "some-model"
"#,
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("must not contain ':'"));
    Ok(())
}

#[test]
fn toml_provider_name_normalized_to_lowercase() -> TestResult {
    let _guard = env_lock()
        .lock()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let temp = tempfile::tempdir()?;
    let env_home = temp.path().join("home");
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::fs::write(
        env_home.join(".evotai").join("evot.toml"),
        r#"
[llm]
provider = "openrouter"

[providers.OpenRouter]
api_key = "toml-key"
base_url = "https://openrouter.ai/api/v1"
model = "anthropic/claude-sonnet-4-20250514"
"#,
    )?;
    // env override should merge into the same normalized provider
    std::fs::write(
        env_home.join(".evotai").join("evot.env"),
        "EVOT_LLM_OPENROUTER_API_KEY=env-key\n",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let config = result?;
    // TOML "OpenRouter" should be normalized to "openrouter"
    assert!(config.providers.contains_key("openrouter"));
    assert!(!config.providers.contains_key("OpenRouter"));
    // env override should have merged into the same provider
    assert_eq!(config.active_llm()?.api_key, "env-key");
    assert_eq!(
        config.active_llm()?.base_url,
        "https://openrouter.ai/api/v1"
    );
    Ok(())
}
