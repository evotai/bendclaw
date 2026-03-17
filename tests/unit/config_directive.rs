use bendclaw::config::BendClawConfig;
use bendclaw::config::DirectiveConfig;

#[test]
fn parse_directive_section() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test.toml");
    std::fs::write(
        &path,
        r#"
node_id = "node-1"

[storage]
databend_api_base_url = "https://test.databend.com"
databend_api_token = "tok"

[directive]
api_base = "https://api.evot.ai"
token = "directive-token"
"#,
    )?;

    let cfg = BendClawConfig::load(path.to_str().unwrap())?;
    let directive = cfg.directive.as_ref().expect("directive should be Some");
    assert_eq!(directive.api_base, "https://api.evot.ai");
    assert_eq!(directive.token, "directive-token");
    Ok(())
}

#[test]
fn serde_roundtrip_with_directive() -> anyhow::Result<()> {
    let mut cfg = BendClawConfig::default();
    cfg.storage.databend_api_base_url = "https://app.databend.com".into();
    cfg.storage.databend_api_token = "tok".into();
    cfg.node_id = "node-1".into();
    cfg.directive = Some(DirectiveConfig {
        api_base: "https://api.evot.ai".into(),
        token: "directive-token".into(),
    });

    let toml_str = toml::to_string(&cfg)?;
    let back: BendClawConfig = toml::from_str(&toml_str)?;
    let directive = back
        .directive
        .as_ref()
        .expect("directive should survive roundtrip");
    assert_eq!(directive.api_base, "https://api.evot.ai");
    assert_eq!(directive.token, "directive-token");
    Ok(())
}

#[test]
fn validate_directive_with_missing_fields_fails() {
    let mut cfg = BendClawConfig::default();
    cfg.storage.databend_api_base_url = "https://app.databend.com".into();
    cfg.storage.databend_api_token = "tok".into();
    cfg.node_id = "node-1".into();
    cfg.directive = Some(DirectiveConfig {
        api_base: "https://api.evot.ai".into(),
        token: String::new(),
    });

    assert!(cfg.validate().is_err());
}
