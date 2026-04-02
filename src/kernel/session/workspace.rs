//! Per-session workspace — unified directory, env, execution, and path safety.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use crate::kernel::variables::Variable;
use crate::types::truncate_head_tail;

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
    variables: Vec<Variable>,
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
            variables: Vec::new(),
            command_idle_timeout,
            max_command_timeout,
            max_output_bytes,
            resolver,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_variables(
        dir: PathBuf,
        cwd: PathBuf,
        safe_env_vars: Vec<String>,
        variables: &[Variable],
        command_idle_timeout: Duration,
        max_command_timeout: Duration,
        max_output_bytes: usize,
        resolver: Arc<dyn PathResolver>,
    ) -> Self {
        let env_vars = variables
            .iter()
            .map(|v| (v.key.clone(), v.value.clone()))
            .collect();
        Self {
            dir,
            cwd,
            safe_env_vars,
            env_vars,
            variables: variables.to_vec(),
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

    /// Run an arbitrary `Command` with streaming stdout/stderr.
    ///
    /// Uses `tokio::join!` to concurrently drain stdout, stderr, and wait for
    /// the child — preventing pipe-buffer deadlocks. Wrapped in a total timeout
    /// as the final safety net.
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

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();
        let max_output = self.max_output_bytes;
        let max_total = self.max_command_timeout;

        let result = tokio::time::timeout(max_total, async {
            let stdout_fut = async {
                let Some(mut reader) = stdout_handle else {
                    return Vec::new();
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    match reader.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if buf.len() < max_output {
                                buf.extend_from_slice(&chunk[..n]);
                            }
                        }
                    }
                }
                // Drain remaining so child doesn't block on write.
                let _ = tokio::io::copy(&mut reader, &mut tokio::io::sink()).await;
                buf
            };

            let stderr_fut = async {
                let Some(mut reader) = stderr_handle else {
                    return Vec::new();
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    match reader.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if buf.len() < max_output {
                                buf.extend_from_slice(&chunk[..n]);
                            }
                        }
                    }
                }
                let _ = tokio::io::copy(&mut reader, &mut tokio::io::sink()).await;
                buf
            };

            let (stdout_buf, stderr_buf, wait_result) =
                tokio::join!(stdout_fut, stderr_fut, child.wait());

            let exit_code = wait_result.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);

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
        })
        .await;

        match result {
            Ok(output) => output,
            Err(_) => {
                // Total timeout exceeded — best effort cleanup only.
                let _ = child.kill().await;
                let _ = child.wait().await;
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

    pub fn variable(&self, key: &str) -> Option<&Variable> {
        self.variables.iter().find(|v| v.key == key)
    }

    pub fn secret_variable_ids(&self) -> Vec<String> {
        self.variables
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
        self.variables
            .iter()
            .filter(|v| v.secret && wanted.contains(v.key.as_str()))
            .map(|v| v.id.clone())
            .collect()
    }
}
