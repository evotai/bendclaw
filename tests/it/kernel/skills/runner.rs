//! Tests for SkillRunner — skill subprocess execution.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::skills::executor::SkillExecutor;
use bendclaw::kernel::skills::runner::SkillRunner;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillFile;
use bendclaw::kernel::skills::skill::SkillRequirements;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;

use crate::mocks::skill::MockSkillCatalog;

fn test_workspace() -> Arc<Workspace> {
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-test-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    Arc::new(Workspace::new(
        dir,
        vec!["PATH".into(), "HOME".into()],
        HashMap::new(),
        Duration::from_secs(10),
        1_048_576,
        Arc::new(SandboxResolver),
    ))
}

// ── skill_read shortcut ──

#[tokio::test]
async fn runner_skill_read_returns_content() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let skill = Skill {
        name: "my-skill".into(),
        version: "1.0".into(),
        description: "desc".into(),
        scope: SkillScope::Global,
        source: SkillSource::Local,
        agent_id: None,
        user_id: None,
        timeout: 10,
        executable: false,
        parameters: vec![],
        content: "This is the skill content.".into(),
        files: vec![],
        requires: None,
    };
    catalog.insert(&skill);

    let runner = SkillRunner::new("a1", "u1", catalog, test_workspace());
    let out = runner
        .execute("skill_read", &["--path".into(), "my-skill".into()])
        .await?;
    assert!(!out.is_error());
    let data = out.data.unwrap();
    assert!(data
        .as_str()
        .unwrap()
        .contains("This is the skill content."));
    Ok(())
}

#[tokio::test]
async fn runner_skill_read_missing_returns_not_found() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let runner = SkillRunner::new("a1", "u1", catalog, test_workspace());
    let out = runner
        .execute("skill_read", &["--path".into(), "nonexistent".into()])
        .await?;
    assert!(!out.is_error());
    let data = out.data.unwrap();
    assert!(data.as_str().unwrap().contains("not found"));
    Ok(())
}

#[tokio::test]
async fn runner_skill_read_no_path_arg_returns_error() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let runner = SkillRunner::new("a1", "u1", catalog, test_workspace());
    // No --path argument at all
    let result = runner.execute("skill_read", &[]).await;
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn runner_skill_read_empty_path_flag_returns_error() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let runner = SkillRunner::new("a1", "u1", catalog, test_workspace());
    // --path with empty string value
    let result = runner
        .execute("skill_read", &["--path".into(), "".into()])
        .await;
    assert!(result.is_err());
    Ok(())
}

// ── unknown skill ──

#[tokio::test]
async fn runner_unknown_skill_returns_error() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let runner = SkillRunner::new("a1", "u1", catalog, test_workspace());
    let result = runner.execute("no-such-skill", &[]).await;
    assert!(result.is_err());
    Ok(())
}

// ── visibility check ──

#[tokio::test]
async fn runner_invisible_skill_returns_error() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let skill = Skill {
        name: "agent-skill".into(),
        version: "1.0".into(),
        description: "desc".into(),
        scope: SkillScope::Agent,
        source: SkillSource::Local,
        agent_id: Some("other-agent".into()),
        user_id: Some("u1".into()),
        timeout: 10,
        executable: true,
        parameters: vec![],
        content: "code".into(),
        files: vec![SkillFile {
            path: "scripts/run.py".into(),
            body: String::new(),
        }],
        requires: None,
    };
    catalog.insert(&skill);

    // runner uses agent_id "a1" but skill belongs to "other-agent"
    let runner = SkillRunner::new("a1", "u1", catalog, test_workspace());
    let result = runner.execute("agent-skill", &[]).await;
    assert!(result.is_err());
    Ok(())
}

// ── no script path ──

#[tokio::test]
async fn runner_skill_without_script_returns_error() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    // Skill with no files → script_path returns None
    let skill = Skill {
        name: "no-script".into(),
        version: "1.0".into(),
        description: "desc".into(),
        scope: SkillScope::Global,
        source: SkillSource::Local,
        agent_id: None,
        user_id: None,
        timeout: 10,
        executable: true,
        parameters: vec![],
        content: "code".into(),
        files: vec![],
        requires: None,
    };
    catalog.insert(&skill);

    let runner = SkillRunner::new("a1", "u1", catalog, test_workspace());
    let result = runner.execute("no-script", &[]).await;
    assert!(result.is_err());
    Ok(())
}

// ── preflight: missing binary ──

#[tokio::test]
async fn runner_preflight_missing_bin_returns_error() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let skill = Skill {
        name: "needs-bin".into(),
        version: "1.0".into(),
        description: "desc".into(),
        scope: SkillScope::Global,
        source: SkillSource::Local,
        agent_id: None,
        user_id: None,
        timeout: 10,
        executable: true,
        parameters: vec![],
        content: "code".into(),
        files: vec![SkillFile {
            path: "scripts/run.sh".into(),
            body: String::new(),
        }],
        requires: Some(SkillRequirements {
            bins: vec!["__nonexistent_binary_xyz__".into()],
            env: vec![],
        }),
    };
    catalog.insert(&skill);

    let runner = SkillRunner::new("a1", "u1", catalog, test_workspace());
    let result = runner.execute("needs-bin", &[]).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("__nonexistent_binary_xyz__") || msg.contains("requires"));
    Ok(())
}

// ── preflight: missing env var ──

#[tokio::test]
async fn runner_preflight_missing_env_returns_error() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let skill = Skill {
        name: "needs-env".into(),
        version: "1.0".into(),
        description: "desc".into(),
        scope: SkillScope::Global,
        source: SkillSource::Local,
        agent_id: None,
        user_id: None,
        timeout: 10,
        executable: true,
        parameters: vec![],
        content: "code".into(),
        files: vec![SkillFile {
            path: "scripts/run.sh".into(),
            body: String::new(),
        }],
        requires: Some(SkillRequirements {
            bins: vec![],
            env: vec!["__NONEXISTENT_ENV_VAR_XYZ__".into()],
        }),
    };
    catalog.insert(&skill);

    // workspace has no user env vars set
    let runner = SkillRunner::new("a1", "u1", catalog, test_workspace());
    let result = runner.execute("needs-env", &[]).await;
    assert!(result.is_err());
    Ok(())
}

// ── actual script execution ──

#[tokio::test]
async fn runner_executes_shell_script_successfully() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("bendclaw-runner-exec-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir)?;

    // Write a real shell script
    let script_path = dir.join("scripts");
    std::fs::create_dir_all(&script_path)?;
    let script_file = script_path.join("run.sh");
    std::fs::write(&script_file, "#!/bin/sh\necho '{\"data\":\"hello\"}'\n")?;

    let catalog = Arc::new(MockSkillCatalog::new());
    let skill = Skill {
        name: "echo-skill".into(),
        version: "1.0".into(),
        description: "desc".into(),
        scope: SkillScope::Global,
        source: SkillSource::Local,
        agent_id: None,
        user_id: None,
        timeout: 10,
        executable: true,
        parameters: vec![],
        content: "code".into(),
        files: vec![SkillFile {
            path: "scripts/run.sh".into(),
            body: String::new(),
        }],
        requires: None,
    };
    catalog.insert(&skill);

    // Override script_path to return the real file path
    // MockSkillCatalog returns the path from files[0].path relative to workspace
    // We need to write the script to the workspace dir
    let workspace = Arc::new(Workspace::new(
        dir.clone(),
        vec!["PATH".into(), "HOME".into()],
        HashMap::new(),
        Duration::from_secs(10),
        1_048_576,
        Arc::new(SandboxResolver),
    ));

    let runner = SkillRunner::new("a1", "u1", catalog, workspace);
    let out = runner.execute("echo-skill", &[]).await?;
    // Script outputs JSON or plain text — either way no error
    assert!(!out.is_error());
    Ok(())
}
