use anyhow::Context as _;
use bendclaw::config::BendClawConfig;

#[test]
fn default_server_config() {
    let cfg = BendClawConfig::default();
    assert_eq!(cfg.server.bind_addr, "127.0.0.1:8787");
}

#[test]
fn default_storage_config() {
    let cfg = BendClawConfig::default();
    assert_eq!(
        cfg.storage.databend_api_base_url,
        "https://api.databend.com/v1"
    );
    assert!(cfg.storage.databend_api_token.is_empty());
    assert!(cfg.storage.databend_warehouse.is_empty());
    assert_eq!(cfg.storage.db_prefix, "bendclaw_v2_");
}

#[test]
fn default_log_config() {
    let cfg = BendClawConfig::default();
    assert_eq!(cfg.log.level, "info");
    assert_eq!(cfg.log.format, "text");
    let expected = std::env::var_os("HOME")
        .map(|home| {
            std::path::PathBuf::from(home)
                .join(".evotai")
                .join("logs")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default();
    assert_eq!(cfg.log.dir, expected);
}

#[test]
fn validate_empty_base_url_fails() {
    let mut cfg = BendClawConfig::default();
    cfg.storage.databend_api_base_url = String::new();
    assert!(cfg.validate().is_err());
}

#[test]
fn validate_empty_token_fails() {
    let mut cfg = BendClawConfig::default();
    cfg.storage.databend_api_base_url = "https://app.databend.com".into();
    assert!(cfg.validate().is_err());
}

#[test]
fn validate_with_base_url_and_token_succeeds() {
    let mut cfg = BendClawConfig::default();
    cfg.storage.databend_api_base_url = "https://app.databend.com".into();
    cfg.storage.databend_api_token = "test-token".into();
    cfg.node_id = "test-instance".into();
    assert!(cfg.validate().is_ok());
}

#[test]
fn load_nonexistent_file_fails() {
    let result = BendClawConfig::load("/nonexistent/path.toml");
    assert!(result.is_err());
}

#[test]
fn load_valid_toml() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test.toml");
    std::fs::write(
        &path,
        r#"
[server]
bind_addr = "0.0.0.0:9000"

[storage]
databend_api_base_url = "https://test.databend.com"
databend_api_token = "my-token"
databend_warehouse = "wh1"
"#,
    )?;
    let cfg = BendClawConfig::load(path.to_str().context("non-UTF8 path")?)?;
    assert_eq!(cfg.server.bind_addr, "0.0.0.0:9000");
    if std::env::var("BENDCLAW_STORAGE_DATABEND_API_BASE_URL").is_err() {
        assert_eq!(
            cfg.storage.databend_api_base_url,
            "https://test.databend.com"
        );
        assert_eq!(cfg.storage.databend_api_token, "my-token");
        assert_eq!(cfg.storage.databend_warehouse, "wh1");
    }
    Ok(())
}

#[test]
fn serde_roundtrip() -> anyhow::Result<()> {
    let mut cfg = BendClawConfig::default();
    cfg.storage.databend_api_base_url = "https://app.databend.com".into();
    cfg.storage.databend_api_token = "tok".into();
    let toml_str = toml::to_string(&cfg)?;
    let back: BendClawConfig = toml::from_str(&toml_str)?;
    assert_eq!(
        back.storage.databend_api_base_url,
        "https://app.databend.com"
    );
    assert_eq!(back.storage.databend_api_token, "tok");
    assert_eq!(back.server.bind_addr, "127.0.0.1:8787");
    Ok(())
}

#[test]
fn hub_config_defaults() {
    let hub = bendclaw::config::HubConfig::default();
    assert_eq!(hub.repo_url, "https://github.com/EvotAI/skills");
    assert_eq!(hub.sync_interval_secs, 86400);
}

#[test]
fn apply_cli_overrides_api_base_url() {
    let mut cfg = BendClawConfig::default();
    let cli = bendclaw::cli::CliOverrides {
        storage_api_base_url: Some("https://cli.databend.com".into()),
        storage_api_token: None,
        storage_warehouse: None,
        bind_addr: None,
        auth_key: None,
        log_level: None,
        log_format: None,
    };
    cfg.apply_cli(&cli);
    assert_eq!(
        cfg.storage.databend_api_base_url,
        "https://cli.databend.com"
    );
}

#[test]
fn apply_cli_overrides_bind_addr() {
    let mut cfg = BendClawConfig::default();
    let cli = bendclaw::cli::CliOverrides {
        storage_api_base_url: None,
        storage_api_token: None,
        storage_warehouse: None,
        bind_addr: Some("0.0.0.0:9999".into()),
        auth_key: None,
        log_level: None,
        log_format: None,
    };
    cfg.apply_cli(&cli);
    assert_eq!(cfg.server.bind_addr, "0.0.0.0:9999");
}

#[test]
fn apply_cli_overrides_log() {
    let mut cfg = BendClawConfig::default();
    let cli = bendclaw::cli::CliOverrides {
        storage_api_base_url: None,
        storage_api_token: None,
        storage_warehouse: None,
        bind_addr: None,
        auth_key: None,
        log_level: Some("debug".into()),
        log_format: Some("json".into()),
    };
    cfg.apply_cli(&cli);
    assert_eq!(cfg.log.level, "debug");
    assert_eq!(cfg.log.format, "json");
}

#[test]
fn apply_cli_none_does_not_override() {
    let mut cfg = BendClawConfig::default();
    let cli = bendclaw::cli::CliOverrides {
        storage_api_base_url: None,
        storage_api_token: None,
        storage_warehouse: None,
        bind_addr: None,
        auth_key: None,
        log_level: None,
        log_format: None,
    };
    cfg.apply_cli(&cli);
    assert_eq!(cfg.server.bind_addr, "127.0.0.1:8787");
    assert_eq!(cfg.log.level, "info");
}
