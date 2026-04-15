//! Linux Landlock LSM sandbox — kernel-level filesystem restrictions.
//!
//! Uses the `landlock` crate to apply filesystem access rules in the child
//! process via `pre_exec`. The parent (evot) process is never restricted.
//! Requires Linux kernel 5.13+.
//!
//! Follows the common Landlock `pre_exec` pattern used by zeptoclaw and
//! RustyClaw: restrictions are applied in the child after fork, before exec.
//! The child is single-threaded at this point so lock contention is not a
//! concern, though the code does perform Rust-level allocations and error
//! formatting which are not strictly async-signal-safe.

use std::path::PathBuf;

use super::SandboxSupport;

/// Check Landlock availability by reading the kernel LSM list.
pub fn check_available() -> SandboxSupport {
    match std::fs::read_to_string("/sys/kernel/security/lsm") {
        Ok(s) if s.contains("landlock") => SandboxSupport::Available,
        Ok(_) => SandboxSupport::Unavailable(
            "Landlock not in kernel LSM list. Enable CONFIG_SECURITY_LANDLOCK".into(),
        ),
        Err(e) => SandboxSupport::Unavailable(format!("Cannot read LSM list: {e}")),
    }
}

/// Apply Landlock restrictions via `pre_exec` on the command.
///
/// The parent process is never restricted — rules only apply to the child
/// after fork, before exec.
pub fn wrap_command(
    cmd: &mut tokio::process::Command,
    allowed_dirs: &[PathBuf],
) -> Result<(), String> {
    let dirs = allowed_dirs.to_vec();

    // SAFETY: Follows common Landlock pre_exec pattern. The child process is
    // single-threaded after fork, so lock contention is not a concern.
    unsafe {
        cmd.pre_exec(move || {
            apply_rules(&dirs)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))
        });
    }

    Ok(())
}

/// Apply Landlock filesystem rules to the current (child) process.
///
/// Allowlist model: only paths explicitly added are accessible.
/// Everything else is denied by omission.
fn apply_rules(allowed_dirs: &[PathBuf]) -> Result<(), String> {
    use landlock::Access;
    use landlock::AccessFs;
    use landlock::PathBeneath;
    use landlock::PathFd;
    use landlock::Ruleset;
    use landlock::RulesetAttr;
    use landlock::RulesetCreatedAttr;
    use landlock::RulesetStatus;
    use landlock::ABI;

    let abi = ABI::V3;

    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_read(abi))
        .map_err(|e| format!("Landlock ruleset read: {e}"))?
        .handle_access(AccessFs::from_write(abi))
        .map_err(|e| format!("Landlock ruleset write: {e}"))?
        .create()
        .map_err(|e| format!("Landlock create: {e}"))?;

    // System paths — read only (+ execute for binaries)
    let system_ro = ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc"];
    for path_str in &system_ro {
        let path = std::path::Path::new(path_str);
        if path.exists() {
            if let Ok(fd) = PathFd::new(path) {
                ruleset = ruleset
                    .add_rule(PathBeneath::new(fd, AccessFs::from_read(abi)))
                    .map_err(|e| format!("Landlock add_rule ro: {e}"))?;
            }
        }
    }

    // Temp — read + write
    let system_rw = ["/tmp", "/var/tmp"];
    for path_str in &system_rw {
        let path = std::path::Path::new(path_str);
        if path.exists() {
            if let Ok(fd) = PathFd::new(path) {
                ruleset = ruleset
                    .add_rule(PathBeneath::new(fd, AccessFs::from_all(abi)))
                    .map_err(|e| format!("Landlock add_rule rw: {e}"))?;
            }
        }
    }

    // Allowed dirs (workspace + extra) — full access
    for dir in allowed_dirs {
        if dir.exists() {
            match PathFd::new(dir) {
                Ok(fd) => {
                    ruleset = ruleset
                        .add_rule(PathBeneath::new(fd, AccessFs::from_all(abi)))
                        .map_err(|e| format!("Landlock add_rule dir: {e}"))?;
                }
                Err(e) => {
                    return Err(format!("Cannot open {:?} for Landlock: {e}", dir));
                }
            }
        }
    }

    // Enforce — irreversible for this process
    match ruleset.restrict_self() {
        Ok(status) => {
            if status.ruleset == RulesetStatus::NotEnforced {
                return Err("Landlock not enforced (kernel too old)".into());
            }
            Ok(())
        }
        Err(e) => Err(format!("Landlock restrict_self: {e}")),
    }
}
