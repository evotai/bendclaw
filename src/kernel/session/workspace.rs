//! Per-session workspace — unified directory, env, execution, and path safety.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use crate::base::truncate_head_tail;
use crate::storage::dal::variable::record::VariableRecord;

// ── Path resolver ──

/// Path resolution strategy — decides which file paths a tool may access.
pub trait PathResolver: Send + Sync + std::fmt::Debug {
    /// Resolve a path. Returns `None` to deny access.
    fn resolve(&self, base_dir: &Path, path: &str) -> Option<PathBuf>;
}

/// Sandbox resolver — paths must stay inside the workspace directory.
#[derive(Debug, Clone)]
pub struct SandboxResolver;

impl PathResolver for SandboxResolver {
    fn resolve(&self, base_dir: &Path, path: &str) -> Option<PathBuf> {
        let candidate = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            base_dir.join(path)
        };

        if let Ok(canonical) = candidate.canonicalize() {
            let ws = base_dir.canonicalize().ok()?;
            if canonical.starts_with(&ws) {
                return Some(canonical);
            }
            return None;
        }

        if let Some(parent) = candidate.parent() {
            if let Ok(parent_canonical) = parent.canonicalize() {
                let ws = base_dir.canonicalize().ok()?;
                if parent_canonical.starts_with(&ws) {
                    return Some(candidate);
                }
            }
        }

        None
    }
}

/// Open resolver — no sandbox restriction.
/// Absolute paths pass through; relative paths resolve against the workspace.
#[derive(Debug, Clone)]
pub struct OpenResolver;

impl PathResolver for OpenResolver {
    fn resolve(&self, base_dir: &Path, path: &str) -> Option<PathBuf> {
        if Path::new(path).is_absolute() {
            Some(PathBuf::from(path))
        } else {
            Some(base_dir.join(path))
        }
    }
}

/// Result of a subprocess execution.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Per-session workspace.
///
/// Created once per session, shared via `Arc` with all tools and skill executors.
/// Owns directory, env isolation, path safety, and command execution.
///
/// Two distinct directories:
/// - `dir`  — where agent-produced files live (downloads, generated code, temp files)
/// - `cwd`  — default working directory for shell/grep/glob (`$HOME` in open mode, `dir` in sandbox)
#[derive(Debug)]
pub struct Workspace {
    /// Agent output directory (workspace storage).
    dir: PathBuf,
    /// Default working directory for shell commands and search tools.
    cwd: PathBuf,
    safe_env_vars: Vec<String>,
    env_vars: HashMap<String, String>,
    variable_records: Vec<VariableRecord>,
    command_idle_timeout: Duration,
    max_command_timeout: Duration,
    max_output_bytes: usize,
    resolver: Arc<dyn PathResolver>,
}

impl Workspace {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dir: PathBuf,
        cwd: PathBuf,
        safe_env_vars: Vec<String>,
        variables: HashMap<String, String>,
        command_idle_timeout: Duration,
        max_command_timeout: Duration,
        max_output_bytes: usize,
        resolver: Arc<dyn PathResolver>,
    ) -> Self {
        Self {
            dir,
            cwd,
            safe_env_vars,
            env_vars: variables,
            variable_records: Vec::new(),
            command_idle_timeout,
            max_command_timeout,
            max_output_bytes,
            resolver,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_variable_records(
        dir: PathBuf,
        cwd: PathBuf,
        safe_env_vars: Vec<String>,
        variable_records: Vec<VariableRecord>,
        command_idle_timeout: Duration,
        max_command_timeout: Duration,
        max_output_bytes: usize,
        resolver: Arc<dyn PathResolver>,
    ) -> Self {
        let env_vars = variable_records
            .iter()
            .map(|v| (v.key.clone(), v.value.clone()))
            .collect();
        Self {
            dir,
            cwd,
            safe_env_vars,
            env_vars,
            variable_records,
            command_idle_timeout,
            max_command_timeout,
            max_output_bytes,
            resolver,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Default working directory for shell commands and search tools.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn command_idle_timeout(&self) -> Duration {
        self.command_idle_timeout
    }

    pub fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }

    /// Build a subprocess environment: `env_clear()` + allowlist + variables.
    pub fn build_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        for var in &self.safe_env_vars {
            if let Ok(val) = std::env::var(var) {
                env.insert(var.clone(), val);
            }
        }
        for (k, v) in &self.env_vars {
            env.insert(k.clone(), v.clone());
        }
        env
    }

    /// Create a `Command` with env isolation and `cwd` as `current_dir`.
    pub fn command(&self, program: &str) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(program);
        cmd.current_dir(&self.cwd);
        cmd.env_clear();
        for (k, v) in self.build_env() {
            cmd.env(&k, &v);
        }
        cmd
    }

    /// Execute a shell command string with idle-timeout streaming.
    pub async fn exec(
        &self,
        shell_command: &str,
        extra: &HashMap<String, String>,
    ) -> CommandOutput {
        let mut cmd = self.command("sh");
        for (k, v) in extra {
            cmd.env(k, v);
        }
        cmd.arg("-c").arg(shell_command);
        self.run_with_idle_timeout(cmd).await
    }

    /// Run an arbitrary `Command` with streaming stdout/stderr and idle timeout.
    /// Wrapped in a total timeout as a final safety net.
    pub async fn run_with_idle_timeout(&self, mut cmd: tokio::process::Command) -> CommandOutput {
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return CommandOutput {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("Failed to spawn: {e}"),
                };
            }
        };

        let Some(mut stdout_handle) = child.stdout.take() else {
            if let Err(e) = child.kill().await {
                tracing::warn!(error = %e, "failed to kill child process");
            }
            if let Err(e) = child.wait().await {
                tracing::warn!(error = %e, "failed to wait on child process");
            }
            return CommandOutput {
                exit_code: -1,
                stdout: String::new(),
                stderr: "Failed to capture stdout".to_string(),
            };
        };
        let Some(mut stderr_handle) = child.stderr.take() else {
            if let Err(e) = child.kill().await {
                tracing::warn!(error = %e, "failed to kill child process");
            }
            if let Err(e) = child.wait().await {
                tracing::warn!(error = %e, "failed to wait on child process");
            }
            return CommandOutput {
                exit_code: -1,
                stdout: String::new(),
                stderr: "Failed to capture stderr".to_string(),
            };
        };
        let idle_timeout = self.command_idle_timeout;
        let max_total = self.max_command_timeout;
        let max_output = self.max_output_bytes;

        let idle_loop = async {
            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();
            let mut stdout_done = false;
            let mut stderr_done = false;

            loop {
                if stdout_done && stderr_done {
                    break;
                }

                let mut buf = [0u8; 4096];
                let mut ebuf = [0u8; 4096];

                tokio::select! {
                    result = stdout_handle.read(&mut buf), if !stdout_done => {
                        match result {
                            Ok(0) => stdout_done = true,
                            Ok(n) => {
                                if stdout_buf.len() < max_output {
                                    stdout_buf.extend_from_slice(&buf[..n]);
                                }
                            }
                            Err(_) => stdout_done = true,
                        }
                    }
                    result = stderr_handle.read(&mut ebuf), if !stderr_done => {
                        match result {
                            Ok(0) => stderr_done = true,
                            Ok(n) => {
                                if stderr_buf.len() < max_output {
                                    stderr_buf.extend_from_slice(&ebuf[..n]);
                                }
                            }
                            Err(_) => stderr_done = true,
                        }
                    }
                    _ = tokio::time::sleep(idle_timeout) => {
                        if let Err(e) = child.kill().await {
                            tracing::warn!(error = %e, "failed to kill timed-out child process");
                        }
                        if let Err(e) = child.wait().await {
                            tracing::warn!(error = %e, "failed to reap timed-out child process");
                        }
                        return CommandOutput {
                            exit_code: -1,
                            stdout: String::from_utf8_lossy(&stdout_buf).into_owned(),
                            stderr: format!(
                                "Command idle timeout after {}s (no output)",
                                idle_timeout.as_secs()
                            ),
                        };
                    }
                }
            }

            let status = child.wait().await;
            let exit_code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);

            let mut stdout = String::from_utf8_lossy(&stdout_buf).into_owned();
            let mut stderr = String::from_utf8_lossy(&stderr_buf).into_owned();

            if stdout.len() > max_output {
                stdout = truncate_head_tail(&stdout, max_output);
            }
            if stderr.len() > max_output {
                stderr = truncate_head_tail(&stderr, max_output);
            }

            CommandOutput {
                exit_code,
                stdout,
                stderr,
            }
        };

        match tokio::time::timeout(max_total, idle_loop).await {
            Ok(output) => output,
            Err(_) => {
                // Total timeout exceeded — force kill and reap.
                if let Err(e) = child.kill().await {
                    tracing::warn!(error = %e, "failed to kill child after total timeout");
                }
                if let Err(e) = child.wait().await {
                    tracing::warn!(error = %e, "failed to reap child after total timeout");
                }
                CommandOutput {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("Command total timeout after {}s", max_total.as_secs()),
                }
            }
        }
    }

    /// Resolve a path for file tools (read/write/edit) — enforces sandbox boundary.
    pub fn resolve_safe_path(&self, path: &str) -> Option<PathBuf> {
        self.resolver.resolve(&self.dir, path)
    }

    /// Resolve a path for search tools (grep/glob/shell).
    /// Relative paths are anchored to `cwd`; absolute paths go through the resolver.
    pub fn resolve_search_path(&self, path: &str) -> Option<PathBuf> {
        if Path::new(path).is_absolute() {
            self.resolver.resolve(&self.dir, path)
        } else {
            let abs = self.cwd.join(path);
            self.resolver.resolve(&self.dir, &abs.to_string_lossy())
        }
    }

    /// Check whether the variables contain a given key (for skill preflight).
    pub fn has_variable(&self, var: &str) -> bool {
        self.env_vars.contains_key(var)
    }

    pub fn variable(&self, key: &str) -> Option<&VariableRecord> {
        self.variable_records.iter().find(|v| v.key == key)
    }

    pub fn secret_variable_ids(&self) -> Vec<String> {
        self.variable_records
            .iter()
            .filter(|v| v.secret)
            .map(|v| v.id.clone())
            .collect()
    }

    pub fn secret_variable_ids_for_keys<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> Vec<String> {
        let wanted: std::collections::HashSet<&str> = keys.into_iter().collect();
        self.variable_records
            .iter()
            .filter(|v| v.secret && wanted.contains(v.key.as_str()))
            .map(|v| v.id.clone())
            .collect()
    }
}
