//! Per-session workspace — unified directory, env, execution, and path safety.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;

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
#[derive(Debug)]
pub struct Workspace {
    dir: PathBuf,
    safe_env_vars: Vec<String>,
    user_env: HashMap<String, String>,
    command_idle_timeout: Duration,
    max_output_bytes: usize,
    resolver: Arc<dyn PathResolver>,
}

impl Workspace {
    pub fn new(
        dir: PathBuf,
        safe_env_vars: Vec<String>,
        user_env: HashMap<String, String>,
        command_idle_timeout: Duration,
        max_output_bytes: usize,
        resolver: Arc<dyn PathResolver>,
    ) -> Self {
        Self {
            dir,
            safe_env_vars,
            user_env,
            command_idle_timeout,
            max_output_bytes,
            resolver,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn command_idle_timeout(&self) -> Duration {
        self.command_idle_timeout
    }

    pub fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }

    /// Build a subprocess environment: `env_clear()` + allowlist + user_env.
    pub fn build_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        for var in &self.safe_env_vars {
            if let Ok(val) = std::env::var(var) {
                env.insert(var.clone(), val);
            }
        }
        for (k, v) in &self.user_env {
            env.insert(k.clone(), v.clone());
        }
        env
    }

    /// Create a `Command` with env isolation and workspace `current_dir` pre-configured.
    pub fn command(&self, program: &str) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(program);
        cmd.current_dir(&self.dir);
        cmd.env_clear();
        for (k, v) in self.build_env() {
            cmd.env(&k, &v);
        }
        cmd
    }

    /// Execute a shell command string with idle-timeout streaming.
    pub async fn exec(&self, shell_command: &str) -> CommandOutput {
        let mut cmd = self.command("sh");
        cmd.arg("-c").arg(shell_command);
        self.run_with_idle_timeout(cmd).await
    }

    /// Run an arbitrary `Command` with streaming stdout/stderr and idle timeout.
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

        let mut stdout_handle = child.stdout.take().unwrap();
        let mut stderr_handle = child.stderr.take().unwrap();
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        let idle_timeout = self.command_idle_timeout;
        let max_output = self.max_output_bytes;

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
                    let _ = child.kill().await;
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
            stdout.truncate(max_output);
            stdout.push_str("\n... [output truncated]");
        }
        if stderr.len() > max_output {
            stderr.truncate(max_output);
            stderr.push_str("\n... [stderr truncated]");
        }

        CommandOutput {
            exit_code,
            stdout,
            stderr,
        }
    }

    /// Resolve a path according to the configured [`PathResolver`] strategy.
    pub fn resolve_safe_path(&self, path: &str) -> Option<PathBuf> {
        self.resolver.resolve(&self.dir, path)
    }

    /// Check whether the user_env contains a given variable (for skill preflight).
    pub fn has_env(&self, var: &str) -> bool {
        self.user_env.contains_key(var)
    }
}
