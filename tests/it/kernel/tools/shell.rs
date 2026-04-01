use std::sync::Arc;

use bendclaw::kernel::tools::builtin::shell::ShellTool;
use bendclaw::kernel::tools::execution::tool_services::NoopSecretUsageSink;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use bendclaw::kernel::Impact;
use serde_json::json;

use crate::mocks::context::test_workspace;

fn make_shell() -> ShellTool {
    ShellTool::new(Arc::new(NoopSecretUsageSink))
}

fn make_ctx(workspace_dir: std::path::PathBuf) -> bendclaw::kernel::tools::ToolContext {
    use ulid::Ulid;
    bendclaw::kernel::tools::ToolContext {
        user_id: format!("u-{}", Ulid::new()).into(),
        session_id: format!("s-{}", Ulid::new()).into(),
        agent_id: "a1".into(),
        run_id: "r-test".into(),
        trace_id: "t-test".into(),
        workspace: test_workspace(workspace_dir),
        is_dispatched: false,
        runtime: bendclaw::kernel::tools::ToolRuntime {
            event_tx: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            tool_call_id: None,
        },
        tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
    }
}

// ── ShellTool execute tests ──

#[tokio::test]
async fn shell_execute_allowed_command() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = make_shell();
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(json!({"command": "echo hello"}), &ctx)
        .await?;
    assert!(result.success);
    assert_eq!(result.output.trim(), "hello");
    Ok(())
}

#[tokio::test]
async fn shell_execute_missing_command_param() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = make_shell();
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool.execute_with_context(json!({}), &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("Missing")));
    Ok(())
}

#[tokio::test]
async fn shell_execute_nonzero_exit_code() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = make_shell();
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(json!({"command": "ls nonexistent_file_xyz"}), &ctx)
        .await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn shell_tool_name_and_schema() {
    let tool = make_shell();
    assert_eq!(tool.name(), "bash");
    let schema = tool.parameters_schema();
    assert!(schema
        .get("properties")
        .and_then(|p| p.get("command"))
        .is_some());
}

// ── classify_impact ──

#[test]
fn classify_impact_destructive_rm() {
    let tool = make_shell();
    assert_eq!(
        tool.classify_impact(&json!({"command": "rm -rf /tmp/foo"})),
        Some(Impact::High)
    );
}

#[test]
fn classify_impact_destructive_git_push() {
    let tool = make_shell();
    assert_eq!(
        tool.classify_impact(&json!({"command": "git push origin main"})),
        Some(Impact::High)
    );
}

#[test]
fn classify_impact_destructive_sudo() {
    let tool = make_shell();
    assert_eq!(
        tool.classify_impact(&json!({"command": "sudo apt-get install foo"})),
        Some(Impact::High)
    );
}

#[test]
fn classify_impact_destructive_docker() {
    let tool = make_shell();
    assert_eq!(
        tool.classify_impact(&json!({"command": "docker run ubuntu"})),
        Some(Impact::High)
    );
}

#[test]
fn classify_impact_readonly_ls() {
    let tool = make_shell();
    assert_eq!(
        tool.classify_impact(&json!({"command": "ls -la"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_cat() {
    let tool = make_shell();
    assert_eq!(
        tool.classify_impact(&json!({"command": "cat README.md"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_git_status() {
    let tool = make_shell();
    assert_eq!(
        tool.classify_impact(&json!({"command": "git status"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_git_log() {
    let tool = make_shell();
    assert_eq!(
        tool.classify_impact(&json!({"command": "git log --oneline"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_medium_cargo_build() {
    let tool = make_shell();
    assert_eq!(
        tool.classify_impact(&json!({"command": "cargo build"})),
        Some(Impact::Medium)
    );
}

#[test]
fn classify_impact_medium_missing_command() {
    let tool = make_shell();
    assert_eq!(tool.classify_impact(&json!({})), Some(Impact::Medium));
}

// ── summarize ──

#[test]
fn summarize_short_command() {
    let tool = make_shell();
    assert_eq!(
        tool.summarize(&json!({"command": "echo hello"})),
        "echo hello"
    );
}

#[test]
fn summarize_missing_command() {
    let tool = make_shell();
    assert_eq!(tool.summarize(&json!({})), "");
}

#[test]
fn summarize_long_command_truncated() {
    let tool = make_shell();
    let long_cmd = "x".repeat(130);
    let result = tool.summarize(&json!({"command": long_cmd}));
    assert!(result.ends_with("..."));
    assert_eq!(result.len(), 120); // 117 chars + "..."
}

#[test]
fn summarize_exactly_120_chars_not_truncated() {
    let tool = make_shell();
    let cmd = "x".repeat(120);
    let result = tool.summarize(&json!({"command": cmd}));
    assert_eq!(result, cmd);
    assert!(!result.ends_with("..."));
}

#[test]
fn summarize_long_multibyte_command_truncated_safely() {
    let tool = make_shell();
    let cmd = "请分析这个项目，给出详细的改进方案。".repeat(20);
    let result = tool.summarize(&json!({"command": cmd}));
    assert!(result.ends_with("..."));
    assert_eq!(result.chars().count(), 120);
}

#[tokio::test]
async fn shell_env_isolation() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = make_shell();
    let ctx = make_ctx(dir.path().to_path_buf());

    std::env::set_var("BENDCLAW_TEST_SECRET", "super_secret");
    let result = tool
        .execute_with_context(json!({"command": "echo $BENDCLAW_TEST_SECRET"}), &ctx)
        .await?;
    assert!(result.success);
    assert_eq!(result.output.trim(), "");
    std::env::remove_var("BENDCLAW_TEST_SECRET");
    Ok(())
}

// ── classify_impact: remaining READONLY_PREFIXES ──

#[test]
fn classify_impact_readonly_head() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "head -5 file.txt"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_tail() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "tail -10 log.txt"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_wc() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "wc -l file.txt"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_grep() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "grep -r pattern src/"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_find() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "find . -name '*.rs'"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_pwd() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "pwd"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_env() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "env"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_git_diff() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "git diff HEAD"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_git_show() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "git show HEAD"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_readonly_git_branch() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "git branch -a"})),
        Some(Impact::Low)
    );
}

#[test]
fn classify_impact_destructive_kubectl() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        make_shell().classify_impact(&serde_json::json!({"command": "kubectl delete pod foo"})),
        Some(Impact::High)
    );
}
