use bendclaw::version::commit_version;
use bendclaw::version::log_version;
use bendclaw::version::BENDCLAW_BUILD_PROFILE;
use bendclaw::version::BENDCLAW_BUILD_TIMESTAMP;
use bendclaw::version::BENDCLAW_GIT_BRANCH;
use bendclaw::version::BENDCLAW_GIT_SHA;
use bendclaw::version::BENDCLAW_RUSTC_VERSION;
use bendclaw::version::BENDCLAW_VERSION;

#[test]
fn commit_version_contains_version() {
    let v = commit_version();
    assert!(v.contains(BENDCLAW_VERSION));
}

#[test]
fn commit_version_contains_parentheses() {
    let v = commit_version();
    assert!(v.contains('('));
    assert!(v.contains(')'));
}

#[test]
fn version_constant_is_semver() {
    let parts: Vec<&str> = BENDCLAW_VERSION.split('.').collect();
    assert_eq!(parts.len(), 3);
    for part in parts {
        assert!(part.parse::<u32>().is_ok());
    }
}

#[test]
fn commit_version_contains_git_sha() {
    let v = commit_version();
    assert!(v.contains(BENDCLAW_GIT_SHA));
}

#[test]
fn commit_version_contains_rustc() {
    let v = commit_version();
    assert!(v.contains(BENDCLAW_RUSTC_VERSION));
}

#[test]
fn commit_version_contains_timestamp() {
    let v = commit_version();
    assert!(v.contains(BENDCLAW_BUILD_TIMESTAMP));
}

#[test]
fn build_constants_not_empty() {
    assert!(!BENDCLAW_GIT_SHA.is_empty());
    assert!(!BENDCLAW_GIT_BRANCH.is_empty());
    assert!(!BENDCLAW_BUILD_TIMESTAMP.is_empty());
    assert!(!BENDCLAW_RUSTC_VERSION.is_empty());
    assert!(!BENDCLAW_BUILD_PROFILE.is_empty());
}

#[test]
fn log_version_does_not_panic() {
    log_version();
}
