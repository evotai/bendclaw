//! OS-level sandbox for bash command execution.
//!
//! Wraps child processes with kernel-enforced filesystem restrictions:
//! - macOS: sandbox-exec with Seatbelt profiles (deny default + allowlist)
//! - Linux: Landlock LSM via pre_exec (kernel 5.13+)

use std::path::PathBuf;

/// Result of sandbox availability check.
pub enum SandboxSupport {
    /// OS sandbox is available and can be applied.
    Available,
    /// OS sandbox is not available on this platform/kernel.
    Unavailable(String),
}

/// Check if OS-level sandbox is available on this platform.
pub fn check_available() -> SandboxSupport {
    platform_check_available()
}

#[cfg(target_os = "macos")]
fn platform_check_available() -> SandboxSupport {
    super::macos::check_available()
}

#[cfg(target_os = "linux")]
fn platform_check_available() -> SandboxSupport {
    super::linux::check_available()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_check_available() -> SandboxSupport {
    SandboxSupport::Unavailable("OS sandbox not supported on this platform".into())
}

/// Wrap a tokio Command with OS-level filesystem restrictions.
///
/// - `allowed_dirs`: directories the child process may read and write.
/// - On macOS: rewrites the command to `sandbox-exec -p <profile> bash -c <cmd>`.
/// - On Linux: sets up a pre_exec hook that applies Landlock rules.
///
/// Returns Err if sandbox cannot be established (caller must not execute).
pub fn wrap_command(
    cmd: &mut tokio::process::Command,
    allowed_dirs: &[PathBuf],
) -> Result<(), String> {
    platform_wrap_command(cmd, allowed_dirs)
}

#[cfg(target_os = "macos")]
fn platform_wrap_command(
    cmd: &mut tokio::process::Command,
    allowed_dirs: &[PathBuf],
) -> Result<(), String> {
    super::macos::wrap_command(cmd, allowed_dirs)
}

#[cfg(target_os = "linux")]
fn platform_wrap_command(
    cmd: &mut tokio::process::Command,
    allowed_dirs: &[PathBuf],
) -> Result<(), String> {
    super::linux::wrap_command(cmd, allowed_dirs)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_wrap_command(
    _cmd: &mut tokio::process::Command,
    _allowed_dirs: &[PathBuf],
) -> Result<(), String> {
    Err("OS sandbox not supported on this platform".into())
}
