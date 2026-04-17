use std::ffi::OsString;
use std::sync::Mutex;
use std::sync::OnceLock;

use evot::conf::thinking_level_from_str;
use evot::conf::Config;
use evot::conf::ProviderKind;
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
    assert_eq!(ProviderKind::default(), ProviderKind::Anthropic);
}

#[test]
fn provider_kind_from_str() -> TestResult {
    assert_eq!(
        ProviderKind::from_str_loose("anthropic")?,
        ProviderKind::Anthropic
    );
    assert_eq!(
        ProviderKind::from_str_loose("openai")?,
        ProviderKind::OpenAi
    );
    assert_eq!(
        ProviderKind::from_str_loose("ANTHROPIC")?,
        ProviderKind::Anthropic
    );
    assert!(ProviderKind::from_str_loose("unknown").is_err());
    Ok(())
}

#[test]
fn config_with_model_overrides_active_provider() {
    let config =
        Config::new(std::path::PathBuf::from("/tmp")).with_model(Some("custom-model".into()));
    assert_eq!(
        config.provider_config(&config.llm.provider).model,
        "custom-model"
    );
}

#[test]
fn config_with_model_none_keeps_default() {
    let config = Config::new(std::path::PathBuf::from("/tmp")).with_model(None);
    assert_eq!(
        config.provider_config(&config.llm.provider).model,
        "claude-sonnet-4-20250514"
    );
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
        "EVOT_ANTHROPIC_API_KEY=file-key\nEVOT_SERVER_PORT=9010\n",
    )?;

    let original_home = std::env::var_os("HOME");
    let original_key = std::env::var_os("EVOT_ANTHROPIC_API_KEY");
    let original_port = std::env::var_os("EVOT_SERVER_PORT");

    std::env::set_var("HOME", &env_home);
    std::env::set_var("EVOT_ANTHROPIC_API_KEY", "process-key");
    std::env::set_var("EVOT_SERVER_PORT", "9020");

    let result = Config::load();

    restore_env_var("HOME", original_home);
    restore_env_var("EVOT_ANTHROPIC_API_KEY", original_key);
    restore_env_var("EVOT_SERVER_PORT", original_port);

    let config = result?;
    assert_eq!(config.active_llm().api_key, "process-key");
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

[anthropic]
api_key = "toml-key"
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
        "export EVOT_ANTHROPIC_MODEL=env-model\nexport EVOT_SERVER_PORT=9010\n",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load().map(|c| c.with_model(Some("cli-model".into())));

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.active_llm().api_key, "toml-key");
    assert_eq!(config.active_llm().model, "cli-model");
    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 9010);
    assert_eq!(config.storage.fs.root_dir, env_home.join("custom-store"));
    Ok(())
}

#[test]
fn load_config_keeps_both_provider_configs() -> TestResult {
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
"#,
    )?;
    std::fs::write(
        env_home.join(".evotai").join("evot.env"),
        r#"
export EVOT_ANTHROPIC_API_KEY=anthropic-key
export EVOT_OPENAI_API_KEY=openai-key
export EVOT_OPENAI_MODEL=gpt-5
"#,
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let mut config = result?;
    assert_eq!(config.active_llm().provider, ProviderKind::Anthropic);
    assert_eq!(config.active_llm().api_key, "anthropic-key");

    config.llm.provider = ProviderKind::OpenAi;
    assert_eq!(config.active_llm().api_key, "openai-key");
    assert_eq!(config.active_llm().model, "gpt-5");
    Ok(())
}

#[test]
fn load_config_normalizes_empty_optional_values() -> TestResult {
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

[anthropic]
api_key = "anthropic-key"
base_url = ""

[storage]
backend = "cloud"

[storage.cloud]
endpoint = "https://cloud.example.com"
api_key = "cloud-key"
workspace = ""
"#,
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.active_llm().base_url, None);
    assert_eq!(config.storage.cloud.workspace, None);
    Ok(())
}

#[test]
fn thinking_level_from_str_parses_all_variants() -> TestResult {
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
    assert_eq!(config.active_llm().thinking_level, ThinkingLevel::Off);
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
thinking_level = "medium"

[llm]
provider = "anthropic"

[anthropic]
api_key = "test-key"
"#,
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.llm.thinking_level, ThinkingLevel::Medium);
    assert_eq!(config.active_llm().thinking_level, ThinkingLevel::Medium);
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
        "EVOT_ANTHROPIC_API_KEY=test-key\nEVOT_THINKING_LEVEL=high\n",
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
thinking_level = "low"

[llm]
provider = "anthropic"

[anthropic]
api_key = "test-key"
"#,
    )?;
    std::fs::write(
        env_home.join(".evotai").join("evot.env"),
        "EVOT_THINKING_LEVEL=high\n",
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
        "EVOT_ANTHROPIC_API_KEY=custom-key\nEVOT_SERVER_PORT=7777\n",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = Config::load_with_env_file(Some(custom_env.to_str().unwrap()));

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.active_llm().api_key, "custom-key");
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
