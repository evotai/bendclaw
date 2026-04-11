use bendclaw::agent::prompt::SystemPrompt;

fn build_prompt(cwd: &str) -> String {
    SystemPrompt::new(cwd)
        .with_system()
        .with_git()
        .with_tools()
        .with_project_context()
        .build()
}

#[test]
fn no_context_files_produces_base_prompt_with_system() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("# System"));
    assert!(prompt.contains("Working directory:"));
    assert!(prompt.contains("Today's date:"));
    assert!(prompt.contains("Platform:"));
    assert!(prompt.contains("Shell:"));
    assert!(prompt.contains("Git repository: no"));
    assert!(!prompt.contains("Project Instructions"));
}

#[test]
fn reads_single_context_file() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    std::fs::write(tmp.path().join("BENDCLAW.md"), "# My Project\nDo X.")
        .expect("failed to write file");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("Project Instructions"));
    assert!(prompt.contains("My Project"));
}

#[test]
fn concatenates_multiple_context_files() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    std::fs::write(tmp.path().join("BENDCLAW.md"), "part one").expect("failed to write file");
    std::fs::write(tmp.path().join("CLAUDE.md"), "part two").expect("failed to write file");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("part one"));
    assert!(prompt.contains("part two"));
}

#[test]
fn skips_empty_context_files() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    std::fs::write(tmp.path().join("BENDCLAW.md"), "   ").expect("failed to write file");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(!prompt.contains("Project Instructions"));
}

#[test]
fn append_is_included() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = SystemPrompt::new(&tmp.path().to_string_lossy())
        .with_system()
        .with_git()
        .with_tools()
        .with_project_context()
        .with_append("Be concise.")
        .build();
    assert!(prompt.contains("Be concise."));
}

#[test]
fn git_repo_detected() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let cwd = tmp.path().to_string_lossy().to_string();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to run git init");

    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to set git email");

    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to set git user");

    let prompt = build_prompt(&cwd);
    assert!(prompt.contains("# Git"));
    assert!(prompt.contains("Git repository: yes"));
    assert!(prompt.contains("Git user: Test User"));
}

#[test]
fn git_repo_shows_branch_and_status() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let cwd = tmp.path().to_string_lossy().to_string();

    for (args, _msg) in [
        (vec!["init", "-b", "main"], "init"),
        (vec!["config", "user.email", "test@test.com"], "email"),
        (vec!["config", "user.name", "Tester"], "name"),
    ] {
        std::process::Command::new("git")
            .args(&args)
            .current_dir(&cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git command failed");
    }

    std::fs::write(tmp.path().join("hello.txt"), "hello").expect("write failed");

    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git add failed");

    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git commit failed");

    let prompt = build_prompt(&cwd);
    assert!(prompt.contains("Current branch: main"));
    assert!(prompt.contains("Recent commits:"));
    assert!(prompt.contains("initial commit"));
}

#[test]
fn sections_are_ordered_system_git_tools() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    let system_pos = prompt.find("# System").expect("missing # System");
    let git_pos = prompt.find("# Git").expect("missing # Git");
    assert!(system_pos < git_pos, "# System should come before # Git");
}

// ---------------------------------------------------------------------------
// Memory tests
// ---------------------------------------------------------------------------

/// Helper: create a temp dir that serves as both HOME and the parent of a
/// project directory. Returns (home, cwd) where cwd = home/project.
fn setup_memory_env() -> (tempfile::TempDir, String) {
    let home = tempfile::TempDir::new().expect("temp dir");
    let project = home.path().join("project");
    std::fs::create_dir_all(&project).expect("create project dir");
    (home, project.to_string_lossy().to_string())
}

/// Mirrors the private `sanitize_for_path` in builder.rs for test setup.
fn sanitize_for_path_test(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Helper: create the bendclaw memory dir for a given home + cwd, write MEMORY.md.
fn write_bendclaw_memory(home: &std::path::Path, cwd: &str, content: &str) {
    let slug = sanitize_for_path_test(cwd);
    let mem_dir = home
        .join(".evotai")
        .join("projects")
        .join(&slug)
        .join("memory");
    std::fs::create_dir_all(&mem_dir).expect("create dir");
    std::fs::write(mem_dir.join("MEMORY.md"), content).expect("write");
}

/// Helper: create the Claude Code memory dir for a given home + cwd, write MEMORY.md.
fn write_claude_memory(home: &std::path::Path, cwd: &str, content: &str) {
    let slug = sanitize_for_path_test(cwd);
    let mem_dir = home
        .join(".claude")
        .join("projects")
        .join(&slug)
        .join("memory");
    std::fs::create_dir_all(&mem_dir).expect("create dir");
    std::fs::write(mem_dir.join("MEMORY.md"), content).expect("write");
}

#[test]
fn memory_section_present_when_empty() {
    let (home, cwd) = setup_memory_env();
    let home_str = home.path().to_string_lossy().to_string();
    let prompt = SystemPrompt::new(&cwd).with_memory_home(&home_str).build();
    assert!(prompt.contains("# Memory"));
    assert!(prompt.contains("currently empty"));
    assert!(!prompt.contains("Claude Code Memory"));
}

#[test]
fn memory_loads_bendclaw_entrypoint() {
    let (home, cwd) = setup_memory_env();
    write_bendclaw_memory(
        home.path(),
        &cwd,
        "- [User prefs](user_prefs.md) — likes Rust",
    );
    let home_str = home.path().to_string_lossy().to_string();
    let prompt = SystemPrompt::new(&cwd).with_memory_home(&home_str).build();
    assert!(prompt.contains("## Bendclaw MEMORY.md"));
    assert!(prompt.contains("likes Rust"));
    assert!(!prompt.contains("currently empty"));
}

#[test]
fn memory_loads_claude_readonly() {
    let (home, cwd) = setup_memory_env();
    write_claude_memory(
        home.path(),
        &cwd,
        "- [Testing](feedback_testing.md) — no mocks",
    );
    let home_str = home.path().to_string_lossy().to_string();
    let prompt = SystemPrompt::new(&cwd).with_memory_home(&home_str).build();
    assert!(prompt.contains("Claude Code Memory (read-only reference)"));
    assert!(prompt.contains("no mocks"));
    assert!(prompt.contains("Do not write to"));
}

#[test]
fn memory_both_sources_ordered() {
    let (home, cwd) = setup_memory_env();
    write_bendclaw_memory(home.path(), &cwd, "- bendclaw entry");
    write_claude_memory(home.path(), &cwd, "- claude entry");
    let home_str = home.path().to_string_lossy().to_string();
    let prompt = SystemPrompt::new(&cwd).with_memory_home(&home_str).build();
    let bc_pos = prompt
        .find("## Bendclaw MEMORY.md")
        .expect("missing bendclaw section");
    let cc_pos = prompt
        .find("## Claude Code Memory")
        .expect("missing claude section");
    assert!(
        bc_pos < cc_pos,
        "Bendclaw MEMORY.md should come before Claude Code Memory"
    );
    assert!(prompt.contains("bendclaw entry"));
    assert!(prompt.contains("claude entry"));
}

#[test]
fn memory_truncates_long_entrypoint() {
    let (home, cwd) = setup_memory_env();
    let long_content: String = (0..300).map(|i| format!("- line {i}\n")).collect();
    write_bendclaw_memory(home.path(), &cwd, &long_content);
    let home_str = home.path().to_string_lossy().to_string();
    let prompt = SystemPrompt::new(&cwd).with_memory_home(&home_str).build();
    assert!(prompt.contains("WARNING"));
    assert!(prompt.contains("truncated"));
    assert!(!prompt.contains("line 250"));
}

#[test]
fn sanitize_for_path_basic() {
    assert_eq!(
        sanitize_for_path_test("/Users/foo/my-project"),
        "-Users-foo-my-project"
    );
    assert_eq!(sanitize_for_path_test("simple"), "simple");
    assert_eq!(sanitize_for_path_test("/a/b/c"), "-a-b-c");
}

#[test]
fn memory_git_subdirs_share_slug() {
    let (home, _cwd) = setup_memory_env();
    let repo = home.path().join("repo");
    std::fs::create_dir_all(&repo).expect("create repo dir");

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git init");

    // Canonicalize to match what `git rev-parse --show-toplevel` returns
    // (macOS resolves /var → /private/var).
    let repo_canonical = repo.canonicalize().expect("canonicalize repo");
    let repo_str = repo_canonical.to_string_lossy().to_string();

    let sub = repo_canonical.join("sub");
    std::fs::create_dir_all(&sub).expect("create subdir");

    write_bendclaw_memory(home.path(), &repo_str, "- shared memory");

    let home_str = home.path().to_string_lossy().to_string();
    let prompt_root = SystemPrompt::new(&repo_str)
        .with_memory_home(&home_str)
        .build();
    let prompt_sub = SystemPrompt::new(&sub.to_string_lossy())
        .with_memory_home(&home_str)
        .build();
    assert!(prompt_root.contains("shared memory"));
    assert!(prompt_sub.contains("shared memory"));
}

#[test]
fn memory_non_git_fallback() {
    let (home, cwd) = setup_memory_env();
    write_bendclaw_memory(home.path(), &cwd, "- non-git memory");
    let home_str = home.path().to_string_lossy().to_string();
    let prompt = SystemPrompt::new(&cwd).with_memory_home(&home_str).build();
    assert!(prompt.contains("non-git memory"));
}

#[test]
fn memory_section_after_project_instructions() {
    let (home, cwd) = setup_memory_env();
    let cwd_path = std::path::Path::new(&cwd);
    std::fs::write(cwd_path.join("BENDCLAW.md"), "# My Project").expect("write");
    let home_str = home.path().to_string_lossy().to_string();
    let prompt = SystemPrompt::new(&cwd)
        .with_project_context()
        .with_memory_home(&home_str)
        .build();
    let proj_pos = prompt
        .find("# Project Instructions")
        .expect("missing project instructions");
    let mem_pos = prompt.find("# Memory").expect("missing memory");
    assert!(
        proj_pos < mem_pos,
        "Project Instructions should come before Memory"
    );
}
