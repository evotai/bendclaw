use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use anyhow::Result;
use bendclaw::kernel::session::workspace::OpenResolver;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;

fn test_ws(dir: std::path::PathBuf) -> Workspace {
    Workspace::new(
        dir,
        vec!["PATH".into(), "HOME".into()],
        HashMap::new(),
        Duration::from_secs(5),
        1_048_576,
        Arc::new(SandboxResolver),
    )
}

// ── resolve_safe_path ──

#[test]
fn resolve_safe_path_relative_inside() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("hello.txt"), "hi")?;
    let ws = test_ws(dir.path().to_path_buf());
    assert!(ws.resolve_safe_path("hello.txt").is_some());
    Ok(())
}

#[test]
fn resolve_safe_path_absolute_inside() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("hello.txt");
    std::fs::write(&file, "hi")?;
    let ws = test_ws(dir.path().to_path_buf());
    assert!(ws
        .resolve_safe_path(file.to_str().context("non-UTF8 path")?)
        .is_some());
    Ok(())
}

#[test]
fn resolve_safe_path_escape_returns_none() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    assert!(ws.resolve_safe_path("../../../etc/passwd").is_none());
    Ok(())
}

#[test]
fn resolve_safe_path_new_file_inside() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    assert!(ws.resolve_safe_path("new_file.txt").is_some());
    Ok(())
}

// ── build_env ──

#[test]
fn build_env_includes_safe_vars() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    let env = ws.build_env();
    // PATH should be inherited from the host
    assert!(env.contains_key("PATH"));
    Ok(())
}

#[test]
fn build_env_includes_user_env() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut user_env = HashMap::new();
    user_env.insert("MY_KEY".into(), "my_value".into());
    let ws = Workspace::new(
        dir.path().to_path_buf(),
        vec!["PATH".into()],
        user_env,
        Duration::from_secs(5),
        1_048_576,
        Arc::new(SandboxResolver),
    );
    let env = ws.build_env();
    assert_eq!(env.get("MY_KEY").context("MY_KEY missing")?, "my_value");
    Ok(())
}

#[test]
fn build_env_user_env_overrides_safe_var() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut user_env = HashMap::new();
    user_env.insert("PATH".into(), "/custom/path".into());
    let ws = Workspace::new(
        dir.path().to_path_buf(),
        vec!["PATH".into()],
        user_env,
        Duration::from_secs(5),
        1_048_576,
        Arc::new(SandboxResolver),
    );
    let env = ws.build_env();
    assert_eq!(env.get("PATH").context("PATH missing")?, "/custom/path");
    Ok(())
}

// ── has_env ──

#[test]
fn has_env_true() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut user_env = HashMap::new();
    user_env.insert("API_KEY".into(), "secret".into());
    let ws = Workspace::new(
        dir.path().to_path_buf(),
        vec![],
        user_env,
        Duration::from_secs(5),
        1_048_576,
        Arc::new(SandboxResolver),
    );
    assert!(ws.has_env("API_KEY"));
    Ok(())
}

#[test]
fn has_env_false() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    assert!(!ws.has_env("NONEXISTENT"));
    Ok(())
}

// ── command ──

#[test]
fn command_sets_current_dir() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    let cmd = ws.command("echo");
    let std_cmd = cmd.as_std();
    assert_eq!(
        std_cmd.get_current_dir().context("no current dir")?,
        dir.path()
    );
    Ok(())
}

// ── exec ──

#[tokio::test]
async fn exec_echo() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    let output = ws.exec("echo hello", &HashMap::new()).await;
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout.trim(), "hello");
    assert!(output.stderr.is_empty());
    Ok(())
}

#[tokio::test]
async fn exec_nonzero_exit() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    let output = ws.exec("exit 42", &HashMap::new()).await;
    assert_eq!(output.exit_code, 42);
    Ok(())
}

#[tokio::test]
async fn exec_env_isolation() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    std::env::set_var("BENDCLAW_WS_TEST_SECRET", "leaked");
    let output = ws
        .exec("echo $BENDCLAW_WS_TEST_SECRET", &HashMap::new())
        .await;
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout.trim(), "");
    std::env::remove_var("BENDCLAW_WS_TEST_SECRET");
    Ok(())
}

#[tokio::test]
async fn exec_user_env_visible() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut user_env = HashMap::new();
    user_env.insert("GREETING".into(), "hi_there".into());
    let ws = Workspace::new(
        dir.path().to_path_buf(),
        vec!["PATH".into()],
        user_env,
        Duration::from_secs(5),
        1_048_576,
        Arc::new(SandboxResolver),
    );
    let output = ws.exec("echo $GREETING", &HashMap::new()).await;
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout.trim(), "hi_there");
    Ok(())
}

#[tokio::test]
async fn exec_idle_timeout() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = Workspace::new(
        dir.path().to_path_buf(),
        vec!["PATH".into()],
        HashMap::new(),
        Duration::from_millis(100),
        1_048_576,
        Arc::new(SandboxResolver),
    );
    let output = ws.exec("sleep 10", &HashMap::new()).await;
    assert_eq!(output.exit_code, -1);
    assert!(output.stderr.contains("idle timeout"));
    Ok(())
}

// ── accessors ──

#[test]
fn dir_returns_workspace_path() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    assert_eq!(ws.dir(), dir.path());
    Ok(())
}

#[test]
fn command_idle_timeout_returns_configured() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    assert_eq!(ws.command_idle_timeout(), Duration::from_secs(5));
    Ok(())
}

#[test]
fn max_output_bytes_returns_configured() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    assert_eq!(ws.max_output_bytes(), 1_048_576);
    Ok(())
}

// ── OpenResolver ──

fn test_ws_open(dir: std::path::PathBuf) -> Workspace {
    Workspace::new(
        dir,
        vec!["PATH".into(), "HOME".into()],
        HashMap::new(),
        Duration::from_secs(5),
        1_048_576,
        Arc::new(OpenResolver),
    )
}

#[test]
fn open_resolver_allows_escape() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws_open(dir.path().to_path_buf());
    assert!(ws.resolve_safe_path("../../../etc/passwd").is_some());
    Ok(())
}

#[test]
fn open_resolver_absolute_path() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws_open(dir.path().to_path_buf());
    let resolved = ws.resolve_safe_path("/tmp/some_file.txt");
    assert_eq!(
        resolved,
        Some(std::path::PathBuf::from("/tmp/some_file.txt"))
    );
    Ok(())
}

#[test]
fn open_resolver_relative_path() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws_open(dir.path().to_path_buf());
    let resolved = ws.resolve_safe_path("foo.txt");
    assert_eq!(resolved, Some(dir.path().join("foo.txt")));
    Ok(())
}

// ── exec with extra env ──

#[tokio::test]
async fn exec_with_env_injects_extra_vars() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    let mut extra = HashMap::new();
    extra.insert("MY_VAR".into(), "injected_value".into());
    let output = ws.exec("echo $MY_VAR", &extra).await;
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout.trim(), "injected_value");
    Ok(())
}

#[tokio::test]
async fn exec_with_env_empty_map_same_as_exec() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    let extra = HashMap::new();
    let output = ws.exec("echo hello", &extra).await;
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout.trim(), "hello");
    Ok(())
}

#[tokio::test]
async fn exec_with_env_does_not_leak_host_env() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    std::env::set_var("BENDCLAW_EXEC_ENV_TEST", "leaked");
    let mut extra = HashMap::new();
    extra.insert("SAFE_VAR".into(), "ok".into());
    let output = ws.exec("echo $BENDCLAW_EXEC_ENV_TEST", &extra).await;
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout.trim(), "");
    std::env::remove_var("BENDCLAW_EXEC_ENV_TEST");
    Ok(())
}

#[tokio::test]
async fn exec_with_env_multiple_vars() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ws = test_ws(dir.path().to_path_buf());
    let mut extra = HashMap::new();
    extra.insert("A".into(), "1".into());
    extra.insert("B".into(), "2".into());
    let output = ws.exec("echo ${A}_${B}", &extra).await;
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout.trim(), "1_2");
    Ok(())
}

#[tokio::test]
async fn exec_with_env_overrides_user_env() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut user_env = HashMap::new();
    user_env.insert("CONFLICT".into(), "original".into());
    let ws = Workspace::new(
        dir.path().to_path_buf(),
        vec!["PATH".into()],
        user_env,
        Duration::from_secs(5),
        1_048_576,
        Arc::new(SandboxResolver),
    );
    let mut extra = HashMap::new();
    extra.insert("CONFLICT".into(), "overridden".into());
    let output = ws.exec("echo $CONFLICT", &extra).await;
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.stdout.trim(), "overridden");
    Ok(())
}
