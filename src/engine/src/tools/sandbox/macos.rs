//! macOS Seatbelt sandbox — allow default, deny $HOME, re-allow allowed dirs.
//!
//! Generates a Seatbelt profile and wraps commands with `sandbox-exec -p <profile>`.
//!
//! Strategy: allow-default + deny $HOME + re-allow specific dirs.
//! Pure deny-default is impractical on macOS because bash/git/cargo need
//! mach ports, IPC, metadata reads, and other operations that are hard to
//! enumerate. Instead we deny the user's home directory and selectively
//! re-allow the directories passed in by the caller.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use super::SandboxSupport;

/// Check sandbox-exec availability.
pub fn check_available() -> SandboxSupport {
    match std::process::Command::new("sandbox-exec")
        .arg("-n")
        .arg("no-network")
        .arg("true")
        .output()
    {
        Ok(o) if o.status.success() => SandboxSupport::Available,
        _ => SandboxSupport::Unavailable("sandbox-exec not available".into()),
    }
}

/// Rewrite the command to run under sandbox-exec.
///
/// Extracts the original program + args, generates a Seatbelt profile,
/// and replaces the command with `sandbox-exec -p <profile> <original>`.
pub fn wrap_command(
    cmd: &mut tokio::process::Command,
    allowed_dirs: &[PathBuf],
) -> Result<(), String> {
    let profile = generate_profile(allowed_dirs);
    let std_cmd = cmd.as_std();
    let original_program = std_cmd.get_program().to_owned();
    let original_args: Vec<_> = std_cmd.get_args().map(|a| a.to_owned()).collect();

    *cmd = tokio::process::Command::new("sandbox-exec");
    cmd.arg("-p").arg(&profile);
    cmd.arg(original_program);
    cmd.args(original_args);

    Ok(())
}

/// Generate a Seatbelt profile: allow default, deny $HOME, re-allow allowed dirs.
fn generate_profile(allowed_dirs: &[PathBuf]) -> String {
    let mut p = String::with_capacity(2048);
    let home = std::env::var("HOME").unwrap_or_default();

    p.push_str("(version 1)\n");
    p.push_str("(allow default)\n");

    // Deny home directory (silently)
    if !home.is_empty() {
        p.push_str(&format!(
            "(deny file-read* file-write* (subpath \"{home}\") (with no-log))\n"
        ));
    }

    // Re-allow each allowed dir — full access
    for dir in allowed_dirs {
        let d = dir.display();
        p.push_str(&format!(
            "(allow file-read* file-write* (subpath \"{d}\"))\n"
        ));
    }

    // Re-allow metadata on ancestor directories inside $HOME
    // (needed for path canonicalization by cargo, git, etc.)
    if !home.is_empty() {
        let home_path = Path::new(&home);
        let mut ancestors = BTreeSet::new();
        for dir in allowed_dirs {
            let mut current = dir.as_path();
            while let Some(parent) = current.parent() {
                if !parent.starts_with(home_path) || parent == home_path {
                    break;
                }
                ancestors.insert(parent.to_path_buf());
                current = parent;
            }
        }
        for ancestor in &ancestors {
            let a = ancestor.display();
            p.push_str(&format!("(allow file-read-metadata (literal \"{a}\"))\n"));
        }
        p.push_str(&format!(
            "(allow file-read-metadata (literal \"{home}\"))\n"
        ));
    }

    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_contains_allow_default() {
        let profile = generate_profile(&[]);
        assert!(profile.contains("(allow default)"));
    }

    #[test]
    fn test_profile_denies_home() {
        let profile = generate_profile(&[]);
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            assert!(profile.contains(&format!(
                "(deny file-read* file-write* (subpath \"{home}\")"
            )));
        }
    }

    #[test]
    fn test_profile_contains_allowed_dirs() {
        let dirs = vec![PathBuf::from("/Users/test/project")];
        let profile = generate_profile(&dirs);
        assert!(
            profile.contains("(allow file-read* file-write* (subpath \"/Users/test/project\"))")
        );
    }

    #[test]
    fn test_profile_ancestor_metadata() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return;
        }
        let dirs = vec![PathBuf::from(format!("{home}/a/b/c"))];
        let profile = generate_profile(&dirs);
        assert!(profile.contains(&format!(
            "(allow file-read-metadata (literal \"{home}/a/b\"))"
        )));
        assert!(profile.contains(&format!(
            "(allow file-read-metadata (literal \"{home}/a\"))"
        )));
        assert!(profile.contains(&format!("(allow file-read-metadata (literal \"{home}\"))")));
    }
}
