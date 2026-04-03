use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bendclaw::kernel::variables::Variable;
use bendclaw::kernel::variables::VariableScope;
use bendclaw::sessions::workspace::SandboxResolver;
use bendclaw::sessions::workspace::Workspace;

fn variable(id: &str, key: &str, value: &str, secret: bool) -> Variable {
    Variable {
        id: id.to_string(),
        key: key.to_string(),
        value: value.to_string(),
        secret,
        revoked: false,
        user_id: String::new(),
        scope: VariableScope::Shared,
        created_by: String::new(),
        last_used_at: None,
        last_used_by: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

#[test]
fn workspace_from_variables_builds_env_and_lookups() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let vars = vec![
        variable("v1", "API_TOKEN", "secret-token", true),
        variable("v2", "LOG_LEVEL", "debug", false),
    ];
    let ws = Workspace::from_variables(
        dir.path().to_path_buf(),
        dir.path().to_path_buf(),
        vec!["PATH".into()],
        &vars,
        Duration::from_secs(5),
        Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    );

    let env = ws.build_env();
    assert_eq!(
        env.get("API_TOKEN").map(String::as_str),
        Some("secret-token")
    );
    assert_eq!(env.get("LOG_LEVEL").map(String::as_str), Some("debug"));
    assert!(ws.has_variable("API_TOKEN"));
    assert_eq!(ws.variable("LOG_LEVEL").map(|v| v.id.as_str()), Some("v2"));
    Ok(())
}

#[test]
fn workspace_secret_variable_helpers_return_expected_ids() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let vars = vec![
        variable("v1", "API_TOKEN", "secret-token", true),
        variable("v2", "LOG_LEVEL", "debug", false),
        variable("v3", "BRAVE_API_KEY", "brave-secret", true),
    ];
    let ws = Workspace::from_variables(
        dir.path().to_path_buf(),
        dir.path().to_path_buf(),
        vec!["PATH".into()],
        &vars,
        Duration::from_secs(5),
        Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    );

    let mut all = ws.secret_variable_ids();
    all.sort();
    assert_eq!(all, vec!["v1".to_string(), "v3".to_string()]);

    let mut filtered = ws.secret_variable_ids_for_keys(["BRAVE_API_KEY", "MISSING"]);
    filtered.sort();
    assert_eq!(filtered, vec!["v3".to_string()]);
    Ok(())
}
