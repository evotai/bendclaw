use std::path::Path;
use std::process::Command;

const PROJECT_CONTEXT_FILES: &[&str] = &["EVOT.md", "CLAUDE.md", "AGENTS.md"];

const AGENT_BEHAVIOR_SECTION: &str = "\
# Output efficiency

IMPORTANT: Go straight to the point. Try the simplest approach first without going in \
circles. Do not overdo it. Be extra concise.

Do NOT ask clarifying questions. When given an unclear or generic instruction, interpret it \
in the most reasonable and comprehensive way and act on it directly. Lead with the answer \
or action, not the reasoning. Do not restate what the user said — just do it.

Keep your text output brief and direct. Skip filler words, preamble, and unnecessary \
transitions. When explaining, include only what is necessary for the user to understand. \
If you can say it in one sentence, don't use three. Focus text output on decisions that \
need the user's input, high-level status updates at natural milestones, and errors or \
blockers that change the plan.

# Doing tasks

 - The user will primarily request you to perform software engineering tasks including \
solving bugs, adding new functionality, refactoring code, explaining code, and more. \
When given an unclear or generic instruction, interpret it in the most reasonable and \
comprehensive way and act on it directly. For example, if the user asks you to change \
\"methodName\" to snake case, find the method in the code and modify it directly — do \
not just reply with \"method_name\".
 - Do not propose changes to code you haven't read. Read first, then modify.
 - Do not create files unless absolutely necessary. Prefer editing existing files.
 - If an approach fails, diagnose why before switching tactics. Escalate to the user \
only when genuinely stuck after investigation, not as a first response to friction.

# Code style

 - Don't add features, refactor code, or make \"improvements\" beyond what was asked.
 - Don't add error handling or validation for scenarios that can't happen.
 - Don't create abstractions for one-time operations. Three similar lines is better \
than a premature abstraction.
 - Before reporting a task complete, verify it works. If you can't verify, say so.
 - Report outcomes faithfully. Never claim success when output shows failures.";

/// Builder for assembling the system prompt.
///
/// ```ignore
/// let prompt = SystemPrompt::new("/path/to/project")
///     .with_agent_behavior()
///     .with_system()
///     .with_git()
///     .with_tools()
///     .with_project_context()
///     .with_memory()
///     .with_append("Be concise.")
///     .build();
/// ```
pub struct SystemPrompt {
    cwd: String,
    sections: Vec<String>,
}

impl SystemPrompt {
    pub fn new(cwd: &str) -> Self {
        Self {
            cwd: cwd.to_string(),
            sections: vec![
                "You are an interactive agent that helps users with software engineering tasks. \
                 Use the instructions below and the tools available to you to assist the user."
                    .into(),
            ],
        }
    }

    /// Append agent behavior guidelines: task execution, code style, tool usage,
    /// output efficiency. This is the core section that drives quality and reduces
    /// unnecessary back-and-forth.
    pub fn with_agent_behavior(mut self) -> Self {
        self.sections.push(AGENT_BEHAVIOR_SECTION.into());
        self
    }

    /// Append system info: working dir, date, platform, shell, OS version.
    pub fn with_system(mut self) -> Self {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let platform = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let shell = detect_shell();

        let mut lines = vec![
            format!("Working directory: {}", self.cwd),
            format!("Today's date: {today}"),
            format!("Platform: {platform} ({arch})"),
            format!("Shell: {shell}"),
        ];

        if let Some(ver) = detect_os_version() {
            lines.push(format!("OS version: {ver}"));
        }

        self.sections
            .push(format!("# System\n\n{}", lines.join("\n")));
        self
    }

    /// Append git repository info: branch, default branch, user, status, recent commits.
    pub fn with_git(mut self) -> Self {
        let is_git = is_git_repo(&self.cwd);

        let mut lines = vec![format!(
            "Git repository: {}",
            if is_git { "yes" } else { "no" }
        )];

        if is_git {
            if let Some(git_info) = collect_git_info(&self.cwd) {
                lines.push(git_info);
            }
        }

        self.sections.push(format!("# Git\n\n{}", lines.join("\n")));
        self
    }

    /// Append available CLI tools (e.g. `gh`).
    pub fn with_tools(mut self) -> Self {
        let mut lines: Vec<String> = Vec::new();

        if has_command("gh") {
            lines.push(
                "GitHub CLI (`gh`): available — prefer `gh` for all GitHub operations \
                 (issues, PRs, API calls, repo info) instead of `curl` or direct API access"
                    .to_string(),
            );
        }

        if !lines.is_empty() {
            self.sections
                .push(format!("# Tools\n\n{}", lines.join("\n")));
        }
        self
    }

    /// Load and append project context from well-known files.
    pub fn with_project_context(mut self) -> Self {
        let mut context = String::new();
        for name in PROJECT_CONTEXT_FILES {
            let path = Path::new(&self.cwd).join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let content = content.trim();
                if !content.is_empty() {
                    if !context.is_empty() {
                        context.push_str("\n\n");
                    }
                    context.push_str(content);
                }
            }
        }
        if !context.is_empty() {
            self.sections
                .push(format!("# Project Instructions\n\n{context}"));
        }
        self
    }

    /// Load memory from evot directories, inject into system prompt.
    /// Global (`~/.evotai/memory/`) and project (`~/.evotai/projects/<slug>/memory/`).
    pub fn with_memory(mut self) -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok();
        if let Some(section) = super::memory::build_section(&self.cwd, home.as_deref()) {
            self.sections.push(section);
        }
        self
    }

    /// Load memory with an explicit home directory override.
    #[doc(hidden)]
    pub fn with_memory_home(mut self, home: &str) -> Self {
        if let Some(section) = super::memory::build_section(&self.cwd, Some(home)) {
            self.sections.push(section);
        }
        self
    }

    /// Temporary compatibility: append Claude Code memory as read-only reference.
    /// Call after `with_memory()`. Safe to remove when Claude compat is no longer needed.
    pub fn with_claude_memory(mut self) -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok();
        if let Some(home) = home.as_deref() {
            self = self.with_claude_memory_home(home);
        }
        self
    }

    /// Temporary compatibility: append Claude Code memory with explicit home override.
    #[doc(hidden)]
    pub fn with_claude_memory_home(mut self, home: &str) -> Self {
        if let Some(section) = super::memory::build_claude_section(&self.cwd, home) {
            self.sections.push(section);
        }
        self
    }

    /// Append arbitrary text (e.g. user-supplied `--append-system-prompt`).
    pub fn with_append(mut self, text: &str) -> Self {
        self.sections.push(text.to_string());
        self
    }

    /// Consume the builder and produce the final prompt string.
    pub fn build(self) -> String {
        self.sections.join("\n\n")
    }
}

// ---------------------------------------------------------------------------
// System helpers
// ---------------------------------------------------------------------------

fn detect_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(String::from))
        .unwrap_or_else(|| "unknown".into())
}

fn detect_os_version() -> Option<String> {
    if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
        run_cmd("uname", &["-sr"])
    } else if cfg!(target_os = "windows") {
        run_cmd("cmd", &["/C", "ver"])
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

fn is_git_repo(cwd: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn collect_git_info(cwd: &str) -> Option<String> {
    let branch = run_git(cwd, &["branch", "--show-current"]).unwrap_or_default();
    let default_branch = detect_default_branch(cwd);
    let user = run_git(cwd, &["config", "user.name"]);
    let log = run_git(cwd, &["log", "--oneline", "-n", "5"]);

    let mut parts = Vec::new();

    if !branch.is_empty() {
        parts.push(format!("Current branch: {branch}"));
    }
    if let Some(main) = default_branch {
        parts.push(format!("Default branch: {main}"));
    }
    if let Some(u) = user {
        parts.push(format!("Git user: {u}"));
    }
    if let Some(l) = log {
        parts.push(format!("Recent commits:\n{l}"));
    }

    if parts.is_empty() {
        return None;
    }
    Some(parts.join("\n"))
}

fn detect_default_branch(cwd: &str) -> Option<String> {
    if let Some(remote_head) = run_git(cwd, &["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        return remote_head
            .strip_prefix("refs/remotes/origin/")
            .map(String::from);
    }
    for candidate in &["main", "master"] {
        let exists = Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{candidate}"),
            ])
            .current_dir(cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if exists {
            return Some((*candidate).to_string());
        }
    }
    None
}

fn run_git(cwd: &str, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(["--no-optional-locks"])
        .args(args)
        .current_dir(cwd)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Tool detection helpers
// ---------------------------------------------------------------------------

fn has_command(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// General helpers
// ---------------------------------------------------------------------------

fn run_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}
