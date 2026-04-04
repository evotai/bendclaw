use std::process::Command;

fn main() {
    // Rebuild when git HEAD changes (new commits, branch switches).
    if std::path::Path::new(".git/HEAD").exists() {
        println!("cargo:rerun-if-changed=.git/HEAD");
    }

    set_env(
        "BENDCLAW_GIT_SHA",
        &git(&["rev-parse", "--short=10", "HEAD"]),
    );
    set_env(
        "BENDCLAW_GIT_BRANCH",
        &git(&["rev-parse", "--abbrev-ref", "HEAD"]),
    );
    set_env("BENDCLAW_GIT_TAG", &git_tag());
    set_env("BENDCLAW_BUILD_TIMESTAMP", &build_timestamp());
    set_env("BENDCLAW_RUSTC_VERSION", &rustc_version());
    set_env(
        "BENDCLAW_BUILD_PROFILE",
        &std::env::var("PROFILE").unwrap_or_else(|_| "unknown".into()),
    );
}

fn set_env(key: &str, value: &str) {
    println!("cargo:rustc-env={key}={value}");
}

fn git(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn git_tag() -> String {
    git(&["describe", "--tags", "--abbrev=0"])
        .trim()
        .to_string()
}

fn build_timestamp() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string()
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}
