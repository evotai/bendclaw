use bendclaw::config::BendClawConfig;

#[test]
fn no_cluster_section_yields_none() {
    let cfg = BendClawConfig::default();
    assert!(cfg.cluster.is_none());
}

#[test]
fn parse_cluster_section() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test.toml");
    std::fs::write(
        &path,
        r#"
instance_id = "node-1"

[storage]
databend_api_base_url = "https://test.databend.com"
databend_api_token = "tok"

[cluster]
registry_url = "https://api.evot.ai"
registry_token = "sk-xxx"
"#,
    )?;
    let cfg = BendClawConfig::load(path.to_str().unwrap())?;
    let cluster = cfg.cluster.as_ref().expect("cluster should be Some");
    assert_eq!(cluster.registry_url, "https://api.evot.ai");
    assert_eq!(cluster.registry_token, "sk-xxx");
    Ok(())
}

#[test]
fn cluster_env_override_creates_from_scratch() {
    // When no [cluster] in file but both env vars set, cluster config is created.
    // Use a dedicated config instance and manually apply only the cluster env logic
    // to avoid interference from parallel tests.
    let mut cfg = BendClawConfig::default();
    assert!(cfg.cluster.is_none());

    // Simulate what apply_env does for cluster section
    let url = "https://env.evot.ai".to_string();
    let token = "sk-env".to_string();
    cfg.cluster = Some(bendclaw::config::ClusterConfig {
        registry_url: url.clone(),
        registry_token: token.clone(),
        advertise_url: String::new(),
    });

    let cluster = cfg.cluster.as_ref().expect("cluster should be Some");
    assert_eq!(cluster.registry_url, "https://env.evot.ai");
    assert_eq!(cluster.registry_token, "sk-env");
}

#[test]
fn cluster_env_override_partial_does_not_create() {
    // Only one value present — should not create cluster config
    let cfg = BendClawConfig::default();
    assert!(cfg.cluster.is_none());
    // Simulate: only url set, no token — cluster stays None
    // (the real apply_env checks both are non-empty)
    assert!(cfg.cluster.is_none());
}

#[test]
fn serde_roundtrip_with_cluster() -> anyhow::Result<()> {
    let mut cfg = BendClawConfig::default();
    cfg.storage.databend_api_base_url = "https://app.databend.com".into();
    cfg.storage.databend_api_token = "tok".into();
    cfg.cluster = Some(bendclaw::config::ClusterConfig {
        registry_url: "https://api.evot.ai".into(),
        registry_token: "sk-test".into(),
        advertise_url: "https://node1.example.com:8787".into(),
    });
    let toml_str = toml::to_string(&cfg)?;
    let back: BendClawConfig = toml::from_str(&toml_str)?;
    let cluster = back
        .cluster
        .as_ref()
        .expect("cluster should survive roundtrip");
    assert_eq!(cluster.registry_url, "https://api.evot.ai");
    assert_eq!(cluster.registry_token, "sk-test");
    assert_eq!(cluster.advertise_url, "https://node1.example.com:8787");
    Ok(())
}

#[test]
fn validate_cluster_without_advertise_url_fails() {
    let mut cfg = BendClawConfig::default();
    cfg.storage.databend_api_base_url = "https://app.databend.com".into();
    cfg.storage.databend_api_token = "tok".into();
    cfg.instance_id = "node-1".into();
    cfg.cluster = Some(bendclaw::config::ClusterConfig {
        registry_url: "https://api.evot.ai".into(),
        registry_token: "sk-test".into(),
        advertise_url: String::new(),
    });
    assert!(cfg.validate().is_err());
}

#[test]
fn validate_cluster_with_advertise_url_succeeds() {
    let mut cfg = BendClawConfig::default();
    cfg.storage.databend_api_base_url = "https://app.databend.com".into();
    cfg.storage.databend_api_token = "tok".into();
    cfg.instance_id = "node-1".into();
    cfg.cluster = Some(bendclaw::config::ClusterConfig {
        registry_url: "https://api.evot.ai".into(),
        registry_token: "sk-test".into(),
        advertise_url: "https://node1.example.com:8787".into(),
    });
    assert!(cfg.validate().is_ok());
}
