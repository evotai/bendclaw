//! Tests for SkillRunner — skill subprocess execution.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::skills::executor::SkillExecutor;
use bendclaw::kernel::skills::runner::SkillRunner;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillFile;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;
use bendclaw::kernel::skills::store::SkillStore;
use bendclaw::storage::VariableRecord;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;
use crate::mocks::skill::test_skill_store;

fn dummy_databases() -> Arc<bendclaw::storage::AgentDatabases> {
    let pool =
        bendclaw::storage::Pool::new("http://localhost:0", "", "default").expect("dummy pool");
    Arc::new(bendclaw::storage::AgentDatabases::new(pool, "test_").unwrap())
}

fn dummy_pool() -> bendclaw::storage::Pool {
    bendclaw::storage::Pool::new("http://localhost:0", "", "default").expect("dummy pool")
}

fn test_workspace() -> Arc<Workspace> {
    test_workspace_with_vars(HashMap::new())
}

fn test_workspace_with_vars(variables: HashMap<String, String>) -> Arc<Workspace> {
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-test-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    Arc::new(Workspace::new(
        dir.clone(),
        dir,
        vec!["PATH".into(), "HOME".into()],
        variables,
        Duration::from_secs(10),
        Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    ))
}

fn test_workspace_with_variable_records(records: Vec<VariableRecord>) -> Arc<Workspace> {
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-test-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    Arc::new(Workspace::from_variable_records(
        dir.clone(),
        dir,
        vec!["PATH".into(), "HOME".into()],
        records,
        Duration::from_secs(10),
        Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    ))
}

fn make_store_with_skill(skill: &Skill) -> Arc<SkillStore> {
    let databases = dummy_databases();
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    let store = test_skill_store(databases, dir);
    store.insert(skill, "a1");
    store
}

fn make_empty_store() -> Arc<SkillStore> {
    let databases = dummy_databases();
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    test_skill_store(databases, dir)
}

fn base_skill(name: &str) -> Skill {
    Skill {
        name: name.into(),
        version: "1.0".into(),
        description: "desc".into(),
        scope: SkillScope::Global,
        source: SkillSource::Local,
        agent_id: None,
        created_by: None,
        timeout: 10,
        executable: false,
        parameters: vec![],
        content: String::new(),
        files: vec![],
        requires: None,
        manifest: None,
    }
}

// ── unknown skill ──

#[tokio::test]
async fn runner_unknown_skill_returns_error() -> Result<()> {
    let store = make_empty_store();
    let runner = SkillRunner::new("a1", "u1", store, test_workspace(), dummy_pool());
    let result = runner.execute("no-such-skill", &[]).await;
    assert!(result.is_err());
    Ok(())
}

// ── visibility check ──

#[tokio::test]
async fn runner_invisible_skill_returns_error() -> Result<()> {
    let mut skill = base_skill("agent-skill");
    skill.scope = SkillScope::Agent;
    skill.agent_id = Some("other-agent".into());
    skill.created_by = Some("u1".into());
    skill.executable = true;
    skill.files = vec![SkillFile {
        path: "scripts/run.py".into(),
        body: String::new(),
    }];
    let store = make_store_with_skill(&skill);

    // runner uses agent_id "a1" but skill belongs to "other-agent"
    let runner = SkillRunner::new("a1", "u1", store, test_workspace(), dummy_pool());
    let result = runner.execute("agent-skill", &[]).await;
    assert!(result.is_err());
    Ok(())
}

// ── no script path ──

#[tokio::test]
async fn runner_skill_without_script_returns_error() -> Result<()> {
    let mut skill = base_skill("no-script");
    skill.executable = true;
    let store = make_store_with_skill(&skill);

    let runner = SkillRunner::new("a1", "u1", store, test_workspace(), dummy_pool());
    let result = runner.execute("no-script", &[]).await;
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn runner_executes_shell_skill_and_parses_json_output() -> Result<()> {
    let mut skill = base_skill("json-skill");
    skill.scope = SkillScope::Agent;
    skill.source = SkillSource::Agent;
    skill.agent_id = Some("a1".into());
    skill.created_by = Some("u1".into());
    skill.executable = true;
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf '{\"data\":\"ok\",\"error\":null}'"
            .into(),
    }];
    let store = make_store_with_skill(&skill);

    let runner = SkillRunner::new("a1", "u1", store, test_workspace(), dummy_pool());
    let output = runner.execute("json-skill", &[]).await?;

    assert_eq!(output.data, Some(serde_json::json!("ok")));
    assert_eq!(output.error, None);
    Ok(())
}

#[tokio::test]
async fn runner_executes_skill_with_required_env_snapshot() -> Result<()> {
    let mut skill = base_skill("env-skill");
    skill.scope = SkillScope::Agent;
    skill.source = SkillSource::Agent;
    skill.agent_id = Some("a1".into());
    skill.created_by = Some("u1".into());
    skill.executable = true;
    skill.requires = Some(bendclaw::kernel::skills::skill::SkillRequirements {
        bins: vec!["bash".into()],
        env: vec!["API_TOKEN".into()],
    });
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf '%s' \"$API_TOKEN\"".into(),
    }];
    let store = make_store_with_skill(&skill);

    let runner = SkillRunner::new(
        "a1",
        "u1",
        store,
        test_workspace_with_vars(HashMap::from([(
            "API_TOKEN".to_string(),
            "secret-token".to_string(),
        )])),
        dummy_pool(),
    );
    let output = runner.execute("env-skill", &[]).await?;

    assert_eq!(output.data, Some(serde_json::json!("secret-token")));
    assert_eq!(output.error, None);
    Ok(())
}

#[tokio::test]
async fn runner_executes_python_skill_with_required_env_snapshot() -> Result<()> {
    let mut skill = base_skill("env-python-skill");
    skill.scope = SkillScope::Agent;
    skill.source = SkillSource::Agent;
    skill.agent_id = Some("a1".into());
    skill.created_by = Some("u1".into());
    skill.executable = true;
    skill.requires = Some(bendclaw::kernel::skills::skill::SkillRequirements {
        bins: vec!["python3".into()],
        env: vec!["API_TOKEN".into()],
    });
    skill.files = vec![SkillFile {
        path: "scripts/run.py".into(),
        body: "import os, sys\nsys.stdin.read()\nprint(os.environ['API_TOKEN'])".into(),
    }];
    let store = make_store_with_skill(&skill);

    let runner = SkillRunner::new(
        "a1",
        "u1",
        store,
        test_workspace_with_vars(HashMap::from([(
            "API_TOKEN".to_string(),
            "python-secret".to_string(),
        )])),
        dummy_pool(),
    );
    let output = runner.execute("env-python-skill", &[]).await?;

    assert_eq!(output.data, Some(serde_json::json!("python-secret")));
    assert_eq!(output.error, None);
    Ok(())
}

#[tokio::test]
async fn runner_missing_required_env_returns_error() -> Result<()> {
    let mut skill = base_skill("needs-env");
    skill.scope = SkillScope::Agent;
    skill.source = SkillSource::Agent;
    skill.agent_id = Some("a1".into());
    skill.created_by = Some("u1".into());
    skill.executable = true;
    skill.requires = Some(bendclaw::kernel::skills::skill::SkillRequirements {
        bins: vec![],
        env: vec!["API_TOKEN".into()],
    });
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\nprintf 'ok'".into(),
    }];
    let store = make_store_with_skill(&skill);

    let runner = SkillRunner::new("a1", "u1", store, test_workspace(), dummy_pool());
    let result = runner.execute("needs-env", &[]).await;

    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn runner_updates_last_used_for_consumed_secret_variables() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(
            sql,
            "UPDATE variables SET last_used_at=NOW() WHERE id IN ('var-secret')"
        );
        Ok(paged_rows(&[], None, None))
    });

    let mut skill = base_skill("secret-skill");
    skill.scope = SkillScope::Agent;
    skill.source = SkillSource::Agent;
    skill.agent_id = Some("a1".into());
    skill.created_by = Some("u1".into());
    skill.executable = true;
    skill.requires = Some(bendclaw::kernel::skills::skill::SkillRequirements {
        bins: vec!["bash".into()],
        env: vec!["API_TOKEN".into()],
    });
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf '%s' \"$API_TOKEN\"".into(),
    }];
    let store = make_store_with_skill(&skill);
    let workspace = test_workspace_with_variable_records(vec![VariableRecord {
        id: "var-secret".into(),
        key: "API_TOKEN".into(),
        value: "secret-token".into(),
        secret: true,
        revoked: false,
        user_id: String::new(),
        scope: String::new(),
        created_by: String::new(),
        last_used_at: None,
        created_at: String::new(),
        updated_at: String::new(),
    }]);
    let runner = SkillRunner::new("a1", "u1", store, workspace, fake.pool());

    let output = runner.execute("secret-skill", &[]).await?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(output.data, Some(serde_json::json!("secret-token")));
    assert_eq!(fake.calls(), vec![FakeDatabendCall::Query {
        sql: "UPDATE variables SET last_used_at=NOW() WHERE id IN ('var-secret')".to_string(),
        database: None,
    }]);
    Ok(())
}
