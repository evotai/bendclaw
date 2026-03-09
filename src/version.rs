pub const BENDCLAW_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BENDCLAW_GIT_SHA: &str = env!("BENDCLAW_GIT_SHA");
pub const BENDCLAW_GIT_BRANCH: &str = env!("BENDCLAW_GIT_BRANCH");
pub const BENDCLAW_GIT_TAG: &str = env!("BENDCLAW_GIT_TAG");
pub const BENDCLAW_BUILD_TIMESTAMP: &str = env!("BENDCLAW_BUILD_TIMESTAMP");
pub const BENDCLAW_RUSTC_VERSION: &str = env!("BENDCLAW_RUSTC_VERSION");
pub const BENDCLAW_BUILD_PROFILE: &str = env!("BENDCLAW_BUILD_PROFILE");

/// Semver-like display string: `v0.1.0-abc1234567(rust-1.80.0-2026-02-19 12:00:00 UTC)`
pub fn commit_version() -> String {
    let tag = if BENDCLAW_GIT_TAG.is_empty() || BENDCLAW_GIT_TAG == "unknown" {
        format!("v{BENDCLAW_VERSION}")
    } else {
        BENDCLAW_GIT_TAG.to_string()
    };
    format!("{tag}-{BENDCLAW_GIT_SHA}({BENDCLAW_RUSTC_VERSION}, {BENDCLAW_BUILD_TIMESTAMP})")
}

/// Log all build metadata at info level.
pub fn log_version() {
    let tag = if BENDCLAW_GIT_TAG.is_empty() || BENDCLAW_GIT_TAG == "unknown" {
        format!("v{BENDCLAW_VERSION}")
    } else {
        BENDCLAW_GIT_TAG.to_string()
    };
    tracing::info!(
        version = %tag,
        commit = BENDCLAW_GIT_SHA,
        branch = BENDCLAW_GIT_BRANCH,
        built = BENDCLAW_BUILD_TIMESTAMP,
        rustc = BENDCLAW_RUSTC_VERSION,
        profile = BENDCLAW_BUILD_PROFILE,
        "starting BendClaw"
    );
}
