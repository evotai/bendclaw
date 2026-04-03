//! Tests for SkillRunner — skill subprocess execution.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bendclaw::execution::skills::SkillExecutor;
use bendclaw::execution::skills::SkillRunner;
use bendclaw::kernel::variables::Variable;
use bendclaw::sessions::workspace::SandboxResolver;
use bendclaw::sessions::workspace::Workspace;
use bendclaw::skills::definition::skill::Skill;
use bendclaw::skills::definition::skill::SkillFile;
use bendclaw::skills::definition::skill::SkillScope;
use bendclaw::skills::definition::skill::SkillSource;
use bendclaw::skills::sync::SkillIndex;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;
use bendclaw_test_harness::mocks::skill::NoopUsageSink;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

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

fn test_workspace_with_variables(variables: &[Variable]) -> Arc<Workspace> {
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-test-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    Arc::new(Workspace::from_variables(
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

fn make_projector_with_skill(skill: &Skill) -> Arc<SkillIndex> {
    make_projector_with_skill_for(skill, "u1")
}

fn make_projector_with_skill_for(skill: &Skill, user_id: &str) -> Arc<SkillIndex> {
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    bendclaw::skills::sources::remote::writer::write_skill(&dir, user_id, skill);
    Arc::new(SkillIndex::new(
        dir,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ))
}

fn make_empty_projector() -> Arc<SkillIndex> {
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    Arc::new(SkillIndex::new(
        dir,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ))
}

fn base_skill(name: &str) -> Skill {
    Skill {
        name: name.into(),
        version: "1.0".into(),
        description: "desc".into(),
        scope: SkillScope::Shared,
        source: SkillSource::Local,
        user_id: String::new(),
        created_by: None,
        last_used_by: None,
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
    let projector = make_empty_projector();
    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        Arc::new(NoopUsageSink),
        test_workspace(),
        dummy_pool(),
    );
    let result = runner.execute("no-such-skill", &[]).await;
    assert!(result.is_err());
    Ok(())
}
// ── visibility check ──

#[tokio::test]
async fn runner_invisible_skill_returns_error() -> Result<()> {
    let mut skill = base_skill("private-skill");
    skill.scope = SkillScope::Private;
    skill.user_id = "other-user".to_string();
    skill.created_by = Some("u1".into());
    skill.last_used_by = None;
    skill.executable = true;
    skill.files = vec![SkillFile {
        path: "scripts/run.py".into(),
        body: String::new(),
    }];
    let projector = make_projector_with_skill(&skill);

    // runner uses user_id "u1" but skill belongs to "other-user"
    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        Arc::new(NoopUsageSink),
        test_workspace(),
        dummy_pool(),
    );
    let result = runner.execute("private-skill", &[]).await;
    assert!(result.is_err());
    Ok(())
}

// ── no script path ──

#[tokio::test]
async fn runner_skill_without_script_returns_error() -> Result<()> {
    let mut skill = base_skill("no-script");
    skill.executable = true;
    let projector = make_projector_with_skill(&skill);

    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        Arc::new(NoopUsageSink),
        test_workspace(),
        dummy_pool(),
    );
    let result = runner.execute("no-script", &[]).await;
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn runner_executes_shell_skill_and_parses_json_output() -> Result<()> {
    let mut skill = base_skill("json-skill");
    skill.scope = SkillScope::Private;
    skill.source = SkillSource::Agent;
    skill.user_id = "u1".to_string();
    skill.created_by = Some("u1".into());
    skill.last_used_by = None;
    skill.executable = true;
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf '{\"data\":\"ok\",\"error\":null}'"
            .into(),
    }];
    let projector = make_projector_with_skill(&skill);

    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        Arc::new(NoopUsageSink),
        test_workspace(),
        dummy_pool(),
    );
    let output = runner.execute("json-skill", &[]).await?;

    assert_eq!(output.data, Some(serde_json::json!("ok")));
    assert_eq!(output.error, None);
    Ok(())
}
#[tokio::test]
async fn runner_executes_skill_with_required_env_snapshot() -> Result<()> {
    let mut skill = base_skill("env-skill");
    skill.scope = SkillScope::Private;
    skill.source = SkillSource::Agent;
    skill.user_id = "u1".to_string();
    skill.created_by = Some("u1".into());
    skill.last_used_by = None;
    skill.executable = true;
    skill.requires = Some(bendclaw::skills::definition::skill::SkillRequirements {
        bins: vec!["bash".into()],
        env: vec!["API_TOKEN".into()],
    });
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf '%s' \"$API_TOKEN\"".into(),
    }];
    let projector = make_projector_with_skill(&skill);

    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        Arc::new(NoopUsageSink),
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
    skill.scope = SkillScope::Private;
    skill.source = SkillSource::Agent;
    skill.user_id = "u1".to_string();
    skill.created_by = Some("u1".into());
    skill.last_used_by = None;
    skill.executable = true;
    skill.requires = Some(bendclaw::skills::definition::skill::SkillRequirements {
        bins: vec!["python3".into()],
        env: vec!["API_TOKEN".into()],
    });
    skill.files = vec![SkillFile {
        path: "scripts/run.py".into(),
        body: "import os, sys\nsys.stdin.read()\nprint(os.environ['API_TOKEN'])".into(),
    }];
    let projector = make_projector_with_skill(&skill);

    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        Arc::new(NoopUsageSink),
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
    skill.scope = SkillScope::Private;
    skill.source = SkillSource::Agent;
    skill.user_id = "u1".to_string();
    skill.created_by = Some("u1".into());
    skill.last_used_by = None;
    skill.executable = true;
    skill.requires = Some(bendclaw::skills::definition::skill::SkillRequirements {
        bins: vec![],
        env: vec!["API_TOKEN".into()],
    });
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\nprintf 'ok'".into(),
    }];
    let projector = make_projector_with_skill(&skill);

    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        Arc::new(NoopUsageSink),
        test_workspace(),
        dummy_pool(),
    );
    let result = runner.execute("needs-env", &[]).await;

    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn runner_updates_last_used_for_consumed_secret_variables() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(
            sql,
            "UPDATE evotai_meta.variables SET last_used_at=NOW(), last_used_by='u1' WHERE id IN ('var-secret')"
        );
        Ok(paged_rows(&[], None, None))
    });

    let mut skill = base_skill("secret-skill");
    skill.scope = SkillScope::Private;
    skill.source = SkillSource::Agent;
    skill.user_id = "u1".to_string();
    skill.created_by = Some("u1".into());
    skill.last_used_by = None;
    skill.executable = true;
    skill.requires = Some(bendclaw::skills::definition::skill::SkillRequirements {
        bins: vec!["bash".into()],
        env: vec!["API_TOKEN".into()],
    });
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf '%s' \"$API_TOKEN\"".into(),
    }];
    let projector = make_projector_with_skill(&skill);
    let workspace = test_workspace_with_variables(&[Variable {
        id: "var-secret".into(),
        key: "API_TOKEN".into(),
        value: "secret-token".into(),
        secret: true,
        revoked: false,
        user_id: String::new(),
        scope: bendclaw::kernel::variables::VariableScope::Shared,
        created_by: String::new(),
        last_used_at: None,
        last_used_by: None,
        created_at: String::new(),
        updated_at: String::new(),
    }]);
    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        Arc::new(NoopUsageSink),
        workspace,
        fake.pool(),
    );

    let output = runner.execute("secret-skill", &[]).await?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(output.data, Some(serde_json::json!("secret-token")));
    assert_eq!(fake.calls(), vec![FakeDatabendCall::Query {
        sql: "UPDATE evotai_meta.variables SET last_used_at=NOW(), last_used_by='u1' WHERE id IN ('var-secret')".to_string(),
        database: None,
    }]);
    Ok(())
}

// ── Subscribed skill execution ──

fn make_projector_with_subscribed_skill(
    skill: &Skill,
    subscriber: &str,
    owner: &str,
) -> Arc<SkillIndex> {
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    bendclaw::skills::sources::remote::writer::write_subscribed_skill(
        &dir, subscriber, owner, skill,
    );
    Arc::new(SkillIndex::new(
        dir,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ))
}

#[tokio::test]
async fn runner_executes_subscribed_skill_via_namespaced_key() -> Result<()> {
    let mut skill = base_skill("shared-tool");
    skill.scope = SkillScope::Shared;
    skill.source = SkillSource::Agent;
    skill.user_id = "alice".to_string();
    skill.created_by = Some("alice".into());
    skill.executable = true;
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body:
            "#!/usr/bin/env bash\ncat >/dev/null\nprintf '{\"data\":\"from-alice\",\"error\":null}'"
                .into(),
    }];
    let projector = make_projector_with_subscribed_skill(&skill, "bob", "alice");

    let runner = SkillRunner::new(
        "a1",
        "bob",
        projector,
        Arc::new(NoopUsageSink),
        test_workspace(),
        dummy_pool(),
    );
    let output = runner.execute("alice/shared-tool", &[]).await?;

    assert_eq!(output.data, Some(serde_json::json!("from-alice")));
    assert_eq!(output.error, None);
    Ok(())
}

#[tokio::test]
async fn runner_subscribed_skill_not_accessible_via_bare_name() -> Result<()> {
    let mut skill = base_skill("shared-tool");
    skill.scope = SkillScope::Shared;
    skill.source = SkillSource::Agent;
    skill.user_id = "alice".to_string();
    skill.executable = true;
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\necho ok".into(),
    }];
    let projector = make_projector_with_subscribed_skill(&skill, "bob", "alice");

    let runner = SkillRunner::new(
        "a1",
        "bob",
        projector,
        Arc::new(NoopUsageSink),
        test_workspace(),
        dummy_pool(),
    );
    let result = runner.execute("shared-tool", &[]).await;

    assert!(
        result.is_err(),
        "subscribed skill should not be accessible via bare name"
    );
    Ok(())
}

// ── UsageSink contract ──

use bendclaw::execution::skills::UsageSink;
use bendclaw::skills::definition::skill::SkillId;
use parking_lot::Mutex;

struct RecordingSink {
    calls: Mutex<Vec<(SkillId, String)>>,
}

impl RecordingSink {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> Vec<(SkillId, String)> {
        self.calls.lock().clone()
    }
}

impl UsageSink for RecordingSink {
    fn touch_used(&self, id: SkillId, agent_id: String) {
        self.calls.lock().push((id, agent_id));
    }
}

#[tokio::test]
async fn runner_calls_usage_sink_on_success() -> Result<()> {
    let mut skill = base_skill("usage-ok");
    skill.scope = SkillScope::Private;
    skill.source = SkillSource::Agent;
    skill.user_id = "u1".to_string();
    skill.created_by = Some("u1".into());
    skill.executable = true;
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf '{\"data\":\"ok\",\"error\":null}'"
            .into(),
    }];
    let projector = make_projector_with_skill(&skill);
    let sink = Arc::new(RecordingSink::new());

    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        sink.clone(),
        test_workspace(),
        dummy_pool(),
    );
    let output = runner.execute("usage-ok", &[]).await?;
    assert!(!output.is_error());

    let calls = sink.calls();
    assert_eq!(
        calls.len(),
        1,
        "touch_used should be called once on success"
    );
    assert_eq!(calls[0].1, "a1");
    Ok(())
}

#[tokio::test]
async fn runner_does_not_call_usage_sink_on_failure() -> Result<()> {
    let mut skill = base_skill("usage-fail");
    skill.scope = SkillScope::Private;
    skill.source = SkillSource::Agent;
    skill.user_id = "u1".to_string();
    skill.created_by = Some("u1".into());
    skill.executable = true;
    skill.files = vec![SkillFile {
        path: "scripts/run.sh".into(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nexit 1".into(),
    }];
    let projector = make_projector_with_skill(&skill);
    let sink = Arc::new(RecordingSink::new());

    let runner = SkillRunner::new(
        "a1",
        "u1",
        projector,
        sink.clone(),
        test_workspace(),
        dummy_pool(),
    );
    let output = runner.execute("usage-fail", &[]).await?;
    assert!(output.is_error());

    let calls = sink.calls();
    assert!(
        calls.is_empty(),
        "touch_used should not be called on failure"
    );
    Ok(())
}
