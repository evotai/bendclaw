use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Mutex;
use std::sync::OnceLock;

use bendclaw::conf::load_config;
use bendclaw::conf::resolve_llm_config;
use bendclaw::conf::ConfigOverrides;
use bendclaw::conf::ProviderKind;
use bendclaw::conf::StorageBackend;

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
fn resolve_config_from_provider_env() -> TestResult {
    let mut vars = HashMap::new();
    vars.insert("BENDCLAW_ANTHROPIC_API_KEY".into(), "file-key".into());
    vars.insert("BENDCLAW_ANTHROPIC_MODEL".into(), "file-model".into());

    let config = resolve_llm_config(&vars, None)?;
    assert_eq!(config.provider, ProviderKind::Anthropic);
    assert_eq!(config.api_key, "file-key");
    assert_eq!(config.model, "file-model");
    Ok(())
}

#[test]
fn resolve_config_cli_model_overrides_provider_env() -> TestResult {
    let mut vars = HashMap::new();
    vars.insert("BENDCLAW_ANTHROPIC_API_KEY".into(), "file-key".into());
    vars.insert("BENDCLAW_ANTHROPIC_MODEL".into(), "file-model".into());

    let config = resolve_llm_config(&vars, Some("cli-model"))?;
    assert_eq!(config.model, "cli-model");
    Ok(())
}

#[test]
fn resolve_config_missing_key_returns_error() {
    let vars = HashMap::new();
    assert!(resolve_llm_config(&vars, None).is_err());
}

#[test]
fn resolve_config_openai_provider() -> TestResult {
    let mut vars = HashMap::new();
    vars.insert("BENDCLAW_LLM_PROVIDER".into(), "openai".into());
    vars.insert("BENDCLAW_OPENAI_API_KEY".into(), "oai-key".into());

    let config = resolve_llm_config(&vars, None)?;
    assert_eq!(config.provider, ProviderKind::OpenAi);
    assert_eq!(config.api_key, "oai-key");
    assert_eq!(config.model, "gpt-4o");
    Ok(())
}

#[test]
fn resolve_config_uses_provider_scoped_env() -> TestResult {
    let mut vars = HashMap::new();
    vars.insert("BENDCLAW_ANTHROPIC_API_KEY".into(), "anthropic-key".into());
    vars.insert("BENDCLAW_ANTHROPIC_MODEL".into(), "anthropic-model".into());
    vars.insert("BENDCLAW_OPENAI_API_KEY".into(), "openai-key".into());
    vars.insert("BENDCLAW_OPENAI_MODEL".into(), "openai-model".into());

    let anthropic = resolve_llm_config(&vars, None)?;
    assert_eq!(anthropic.provider, ProviderKind::Anthropic);
    assert_eq!(anthropic.api_key, "anthropic-key");
    assert_eq!(anthropic.model, "anthropic-model");

    vars.insert("BENDCLAW_LLM_PROVIDER".into(), "openai".into());
    let openai = resolve_llm_config(&vars, None)?;
    assert_eq!(openai.provider, ProviderKind::OpenAi);
    assert_eq!(openai.api_key, "openai-key");
    assert_eq!(openai.model, "openai-model");
    Ok(())
}

#[test]
fn resolve_config_default_model_per_provider() -> TestResult {
    let mut vars = HashMap::new();
    vars.insert("BENDCLAW_ANTHROPIC_API_KEY".into(), "key".into());

    let config = resolve_llm_config(&vars, None)?;
    assert_eq!(config.model, "claude-sonnet-4-20250514");
    Ok(())
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
        env_home.join(".evotai").join("bendclaw.env"),
        "BENDCLAW_ANTHROPIC_API_KEY=file-key\nBENDCLAW_SERVER_PORT=9010\n",
    )?;

    let original_home = std::env::var_os("HOME");
    let original_key = std::env::var_os("BENDCLAW_ANTHROPIC_API_KEY");
    let original_port = std::env::var_os("BENDCLAW_SERVER_PORT");

    std::env::set_var("HOME", &env_home);
    std::env::set_var("BENDCLAW_ANTHROPIC_API_KEY", "process-key");
    std::env::set_var("BENDCLAW_SERVER_PORT", "9020");

    let result = load_config(ConfigOverrides::new(None, None));

    restore_env_var("HOME", original_home);
    restore_env_var("BENDCLAW_ANTHROPIC_API_KEY", original_key);
    restore_env_var("BENDCLAW_SERVER_PORT", original_port);

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
        env_home.join(".evotai").join("bendclaw.toml"),
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
        env_home.join(".evotai").join("bendclaw.env"),
        "export BENDCLAW_ANTHROPIC_MODEL=env-model\nexport BENDCLAW_SERVER_PORT=9010\n",
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = load_config(ConfigOverrides::new(Some("cli-model".into()), None));

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
        env_home.join(".evotai").join("bendclaw.toml"),
        r#"
[llm]
provider = "anthropic"
"#,
    )?;
    std::fs::write(
        env_home.join(".evotai").join("bendclaw.env"),
        r#"
export BENDCLAW_ANTHROPIC_API_KEY=anthropic-key
export BENDCLAW_OPENAI_API_KEY=openai-key
export BENDCLAW_OPENAI_MODEL=gpt-5
"#,
    )?;

    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &env_home);

    let result = load_config(ConfigOverrides::new(None, None));

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
        env_home.join(".evotai").join("bendclaw.toml"),
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

    let result = load_config(ConfigOverrides::new(None, None));

    restore_env_var("HOME", original_home);

    let config = result?;
    assert_eq!(config.active_llm().base_url, None);
    assert_eq!(config.storage.cloud.workspace, None);
    Ok(())
}
