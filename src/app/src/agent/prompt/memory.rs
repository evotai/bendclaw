//! Memory path resolution, MemoryTool construction, and system prompt section building.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use evot_engine::tools::memory::MemoryTool;

const MAX_SANITIZED_LENGTH: usize = 200;
const MEMORY_PROMPT: &str = include_str!("memory.md");
const MAX_ENTRYPOINT_LINES: usize = 200;
const MAX_ENTRYPOINT_BYTES: usize = 25_000;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Resolved memory directory paths (global + project only).
pub struct MemoryPaths {
    /// `~/.evotai/memory/`
    pub global_dir: PathBuf,
    /// `~/.evotai/projects/<slug>/memory/`
    pub project_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// MemoryTool construction
// ---------------------------------------------------------------------------

/// Construct a `MemoryTool` for the given working directory.
/// Returns `None` if the home directory cannot be determined.
pub fn load_memory_tool(cwd: &str) -> Option<MemoryTool> {
    let paths = resolve_paths(cwd)?;
    Some(MemoryTool::new(paths.global_dir, paths.project_dir))
}

/// Return the memory directories for sandbox allowlist purposes.
/// Returns an empty vec if the home directory cannot be determined.
pub fn resolve_memory_dirs(cwd: &str) -> Vec<PathBuf> {
    match resolve_paths(cwd) {
        Some(paths) => vec![paths.global_dir, paths.project_dir],
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// System prompt section builders
// ---------------------------------------------------------------------------

/// Build the `# Memory` section for the system prompt.
/// Returns `None` if `home` is not provided.
pub fn build_section(cwd: &str, home: Option<&str>) -> Option<String> {
    let home = home?;
    let paths = resolve_paths_with_home(cwd, home);

    ensure_dir(&paths.global_dir);
    ensure_dir(&paths.project_dir);

    let global_content = read_entrypoint(&paths.global_dir);
    let project_content = read_entrypoint(&paths.project_dir);

    let global_display = paths.global_dir.display();
    let project_display = paths.project_dir.display();

    let mut section = format!(
        "# Memory\n\n\
         You have a two-layer persistent memory system.\n\
         - Global: `{global_display}`\n\
         - Project: `{project_display}`\n\n\
         Use the `memory` tool to manage it — do not write memory files directly.\n\n\
         {MEMORY_PROMPT}\n\n\
         ## Global MEMORY.md\n\n"
    );

    match global_content {
        Some(content) => section.push_str(&content),
        None => section.push_str("Your global MEMORY.md is currently empty."),
    }

    section.push_str("\n\n## Project MEMORY.md\n\n");
    match project_content {
        Some(content) => section.push_str(&content),
        None => section.push_str("Your project MEMORY.md is currently empty."),
    }

    Some(section)
}

/// Build the Claude Code memory section (read-only reference).
/// Temporary compatibility — safe to remove when Claude compat is dropped.
pub fn build_claude_section(cwd: &str, home: &str) -> Option<String> {
    let dir = claude_memory_dir(cwd, home)?;
    let content = read_entrypoint(&dir)?;
    Some(format!(
        "## Claude Code Memory (read-only reference)\n\n\
         The following memory index was loaded from a Claude Code memory directory \
         for reference only. Do not write to, update, or reorganize that directory.\n\n\
         {content}"
    ))
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

/// Resolve memory paths for the given working directory.
/// Returns `None` if the home directory cannot be determined.
fn resolve_paths(cwd: &str) -> Option<MemoryPaths> {
    let home = resolve_home()?;
    Some(resolve_paths_with_home(cwd, &home))
}

/// Resolve memory paths with an explicit home directory.
pub fn resolve_paths_with_home(cwd: &str, home: &str) -> MemoryPaths {
    MemoryPaths {
        global_dir: global_memory_dir(home),
        project_dir: project_memory_dir(cwd, home),
    }
}

fn global_memory_dir(home: &str) -> PathBuf {
    PathBuf::from(home).join(".evotai").join("memory")
}

fn project_memory_dir(cwd: &str, home: &str) -> PathBuf {
    let slug = project_slug(cwd);
    PathBuf::from(home)
        .join(".evotai")
        .join("projects")
        .join(slug)
        .join("memory")
}

/// Resolve `~/.claude/projects/<slug>/memory/` if it exists on disk.
fn claude_memory_dir(cwd: &str, home: &str) -> Option<PathBuf> {
    let slug = project_slug(cwd);
    let dir = PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(slug)
        .join("memory");
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

fn resolve_home() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
}

fn project_slug(cwd: &str) -> String {
    sanitize_for_path(&find_git_root(cwd))
}

fn find_git_root(cwd: &str) -> String {
    Command::new("git")
        .args(["--no-optional-locks", "rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cwd.to_string())
}

fn sanitize_for_path(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    if sanitized.len() <= MAX_SANITIZED_LENGTH {
        return sanitized;
    }
    format!(
        "{}-{}",
        &sanitized[..MAX_SANITIZED_LENGTH],
        stable_hash(name)
    )
}

/// FNV-1a hash for stable cross-platform path hashing.
fn stable_hash(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// MEMORY.md reading helpers
// ---------------------------------------------------------------------------

fn ensure_dir(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
}

/// Read `MEMORY.md` from a directory. Returns `None` if missing or empty.
fn read_entrypoint(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("MEMORY.md")).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_entrypoint(trimmed))
}

fn truncate_entrypoint(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let line_over = lines.len() > MAX_ENTRYPOINT_LINES;
    let byte_over = content.len() > MAX_ENTRYPOINT_BYTES;

    if !line_over && !byte_over {
        return content.to_string();
    }

    let mut result = if line_over {
        lines[..MAX_ENTRYPOINT_LINES].join("\n")
    } else {
        content.to_string()
    };

    if result.len() > MAX_ENTRYPOINT_BYTES {
        let safe = truncate_to_char_boundary(&result, MAX_ENTRYPOINT_BYTES);
        let cut = safe.rfind('\n').unwrap_or(safe.len());
        result.truncate(cut);
    }

    result.push_str(
        "\n\n> WARNING: MEMORY.md exceeded the load limit and was truncated. \
         Keep index entries concise and move details into topic files.",
    );
    result
}

fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    &s[..s.floor_char_boundary(max_bytes)]
}
