use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use super::render::DIM;
use super::render::GREEN;
use super::render::RED;
use super::render::RESET;
use super::render::YELLOW;
use crate::agent::Agent;
use crate::agent::ForkRequest;
use crate::agent::RunEventPayload;
use crate::conf::paths;
use crate::error::BendclawError;
use crate::error::Result;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn handle_skill_command(input: &str, agent: &Arc<Agent>) -> Result<()> {
    let args = input.strip_prefix("/skill").unwrap_or("").trim();

    if args.is_empty() || args == "list" {
        return skill_list();
    }

    if let Some(source) = args.strip_prefix("install ") {
        let source = source.trim();
        if source.is_empty() {
            eprintln!("{RED}  usage: /skill install <owner/repo or github-url>{RESET}\n");
            return Ok(());
        }
        return skill_install(source, agent).await;
    }

    if let Some(name) = args.strip_prefix("remove ") {
        let name = name.trim();
        if name.is_empty() {
            eprintln!("{RED}  usage: /skill remove <name>{RESET}\n");
            return Ok(());
        }
        return skill_remove(name);
    }

    eprintln!("{RED}  unknown subcommand: /skill {args}{RESET}");
    eprintln!("{DIM}  usage: /skill [list | install <source> | remove <name>]{RESET}\n");
    Ok(())
}

// ---------------------------------------------------------------------------
// /skill list
// ---------------------------------------------------------------------------

fn skill_list() -> Result<()> {
    let skills_dir = paths::skills_dir()?;
    if !skills_dir.exists() {
        println!("{DIM}  no skills installed{RESET}\n");
        return Ok(());
    }

    let entries = fs::read_dir(&skills_dir)
        .map_err(|e| BendclawError::Cli(format!("failed to read skills dir: {e}")))?;

    let mut found = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let description = read_skill_description(&skill_md).unwrap_or_default();
        if description.is_empty() {
            println!("  {name}");
        } else {
            println!("  {name}{DIM}  — {description}{RESET}");
        }
        found = true;
    }

    if !found {
        println!("{DIM}  no skills installed{RESET}");
    }
    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// /skill install
// ---------------------------------------------------------------------------

async fn skill_install(source: &str, agent: &Arc<Agent>) -> Result<()> {
    check_gh_available()?;

    let src = parse_github_source(source)?;
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| BendclawError::Cli(format!("failed to create temp dir: {e}")))?;
    let clone_dir = tmp_dir.path().join("repo");

    // Clone with progress hint
    println!("{DIM}  cloning {}{RESET}", src.repo);
    clone_repo(&src.repo, src.git_ref.as_deref(), &clone_dir)?;

    let skills_dir = paths::skills_dir()?;
    fs::create_dir_all(&skills_dir)
        .map_err(|e| BendclawError::Cli(format!("failed to create skills dir: {e}")))?;

    // Determine what to install
    let installed = if let Some(ref subpath) = src.subpath {
        let sub_dir = clone_dir.join(subpath);
        if !sub_dir.is_dir() {
            return Err(BendclawError::Cli(format!(
                "subpath \"{}\" not found in repository",
                subpath.display()
            )));
        }
        let skill_md = sub_dir.join("SKILL.md");
        if !skill_md.exists() {
            return Err(BendclawError::Cli(format!(
                "no SKILL.md found in {}",
                subpath.display()
            )));
        }
        let name = sub_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        install_skill_dir(&sub_dir, &skills_dir, &name)?;
        vec![name]
    } else if clone_dir.join("SKILL.md").exists() {
        // Single skill repo
        let name = repo_name(&src.repo);
        install_skill_dir(&clone_dir, &skills_dir, &name)?;
        vec![name]
    } else {
        // Multi-skill repo: scan top-level subdirectories
        let mut names = Vec::new();
        let entries = fs::read_dir(&clone_dir)
            .map_err(|e| BendclawError::Cli(format!("failed to read cloned repo: {e}")))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if !path.join("SKILL.md").exists() {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            install_skill_dir(&path, &skills_dir, &name)?;
            names.push(name);
        }
        if names.is_empty() {
            return Err(BendclawError::Cli(
                "no skills found in repository (no SKILL.md in root or subdirectories)".into(),
            ));
        }
        names.sort();
        names
    };

    // tmp_dir is automatically cleaned up on drop

    // Print results and show setup + safety guide via LLM (single skill only)
    for name in &installed {
        println!("{GREEN}  ✓ installed skill: {name}{RESET}");
    }
    if installed.len() == 1 {
        let skill_dir = skills_dir.join(&installed[0]);
        print_setup_guide(agent, &installed[0], &skill_dir).await;
    } else if installed.len() > 1 {
        println!(
            "{DIM}  installed {} skills; use the skill tool to explore each one{RESET}",
            installed.len()
        );
    }
    println!();
    Ok(())
}

/// Copy a skill directory to the target skills dir, excluding `.git`.
fn install_skill_dir(src: &Path, skills_dir: &Path, name: &str) -> Result<()> {
    let target = skills_dir.join(name);
    // Remove existing
    if target.exists() {
        fs::remove_dir_all(&target)
            .map_err(|e| BendclawError::Cli(format!("failed to remove existing skill: {e}")))?;
    }
    copy_dir_excluding_git(src, &target)?;
    Ok(())
}

pub fn copy_dir_excluding_git(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)
        .map_err(|e| BendclawError::Cli(format!("failed to create dir {}: {e}", dst.display())))?;

    let entries = fs::read_dir(src)
        .map_err(|e| BendclawError::Cli(format!("failed to read {}: {e}", src.display())))?;

    for entry in entries {
        let entry = entry.map_err(|e| BendclawError::Cli(format!("failed to read entry: {e}")))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip .git directory
        if name_str == ".git" {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&name);

        if src_path.is_dir() {
            copy_dir_excluding_git(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|e| {
                BendclawError::Cli(format!(
                    "failed to copy {} → {}: {e}",
                    src_path.display(),
                    dst_path.display()
                ))
            })?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Post-install setup + safety guide via LLM
// ---------------------------------------------------------------------------

async fn print_setup_guide(agent: &Arc<Agent>, skill_name: &str, skill_dir: &Path) {
    let content = collect_skill_context(skill_dir);
    if content.is_empty() {
        return;
    }

    // Truncate to avoid blowing up context (safe char boundary)
    let limit = 12000.min(content.len());
    let content = &content[..content.floor_char_boundary(limit)];

    let system_prompt = format!(
        "A skill named \"{skill_name}\" was just installed.\n\
         Analyze the provided files and output TWO short sections:\n\n\
         ## Configuration\n\
         What does the user need to configure before using this skill?\n\
         - List every required config item: env vars, config files, API tokens, CLI tools\n\
         - For each item, show the exact key/file name and where to get the value\n\
         - If there is an env template, list every key with a one-line explanation\n\
         - If nothing to configure, say \"None.\"\n\n\
         ## Security\n\
         What should the user be aware of?\n\
         - Scripts that will be executed\n\
         - External network access (APIs, endpoints)\n\
         - Sensitive data accessed (tokens, credentials)\n\
         - Potentially destructive operations\n\
         - If nothing notable, say \"None.\"\n\n\
         Rules:\n\
         - Bullet points only, be concise\n\
         - Focus on what needs to be configured and what to watch out for\n\
         - Do NOT explain what the skill does or how to use it\n\
         - Do NOT read any files or execute any commands\n\
         - Reply in the same language as the skill definition\n\n\
         {content}"
    );

    let request = ForkRequest { system_prompt };
    let mut side = match agent.fork(request) {
        Ok(s) => s,
        Err(_) => {
            println!("{DIM}  could not analyze setup/safety notes{RESET}");
            return;
        }
    };

    let prompt = format!(
        "List the configuration requirements and security notes for the \"{skill_name}\" skill."
    );
    let mut stream = match side.query(&prompt).await {
        Ok(s) => s,
        Err(_) => {
            println!("{DIM}  could not analyze setup/safety notes{RESET}");
            return;
        }
    };

    println!();
    println!("{DIM}  analyzing skill...{RESET}");
    let mut has_output = false;
    while let Some(event) = stream.next().await {
        if let RunEventPayload::AssistantDelta {
            delta: Some(text), ..
        } = &event.payload
        {
            if !has_output {
                has_output = true;
            }
            print!("{DIM}{text}{RESET}");
        }
    }
    if has_output {
        println!();
    }
}

/// Collect context files from a skill directory for LLM analysis.
///
/// Reads SKILL.md, .env / .env.example / .env.local.example, and README.md
/// (if they exist), and formats them as labeled XML blocks.
fn collect_skill_context(skill_dir: &Path) -> String {
    let candidates: &[(&str, &str)] = &[
        ("SKILL.md", "skill_definition"),
        (".env", "env_template"),
        (".env.example", "env_template"),
        (".env.local.example", "env_template"),
        ("README.md", "readme"),
    ];

    let mut parts = Vec::new();
    let mut seen_tags = std::collections::HashSet::new();

    for (filename, tag) in candidates {
        let path = skill_dir.join(filename);
        if let Ok(content) = fs::read_to_string(&path) {
            if content.trim().is_empty() {
                continue;
            }
            // Avoid duplicate tags (e.g. if both .env and .env.example exist)
            let label = if seen_tags.contains(*tag) {
                format!("{tag}_{filename}")
            } else {
                seen_tags.insert(*tag);
                tag.to_string()
            };
            parts.push(format!("<{label}>\n{content}\n</{label}>"));
        }
    }

    parts.join("\n\n")
}

// ---------------------------------------------------------------------------
// /skill remove
// ---------------------------------------------------------------------------

fn skill_remove(name: &str) -> Result<()> {
    if !is_valid_skill_name(name) {
        return Err(BendclawError::Cli(format!(
            "invalid skill name: \"{name}\" — only [A-Za-z0-9._-] allowed"
        )));
    }

    let skills_dir = paths::skills_dir()?;
    let target = skills_dir.join(name);

    if !target.exists() {
        eprintln!("{YELLOW}  skill \"{name}\" not found{RESET}\n");
        return Ok(());
    }

    fs::remove_dir_all(&target)
        .map_err(|e| BendclawError::Cli(format!("failed to remove skill \"{name}\": {e}")))?;

    println!("{DIM}  removed skill: {name}{RESET}\n");
    Ok(())
}

pub fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

// ---------------------------------------------------------------------------
// GitHub source parsing
// ---------------------------------------------------------------------------

pub struct GitHubSource {
    pub repo: String,
    pub git_ref: Option<String>,
    pub subpath: Option<PathBuf>,
}

/// Parse a GitHub source string into structured fields.
///
/// Supported formats:
///   owner/repo
///   https://github.com/owner/repo
///   https://github.com/owner/repo/tree/<ref>/<path>
pub fn parse_github_source(input: &str) -> Result<GitHubSource> {
    // Full GitHub URL
    if input.starts_with("https://github.com/") || input.starts_with("http://github.com/") {
        return parse_github_url(input);
    }

    // owner/repo shorthand
    let parts: Vec<&str> = input.split('/').collect();
    if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() && !parts[0].contains('.') {
        return Ok(GitHubSource {
            repo: input.to_string(),
            git_ref: None,
            subpath: None,
        });
    }

    Err(BendclawError::Cli(format!(
        "invalid source: \"{input}\"\n  expected: owner/repo or https://github.com/owner/repo[/tree/ref/path]"
    )))
}

fn parse_github_url(url: &str) -> Result<GitHubSource> {
    // Strip scheme and host
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .unwrap_or(url);

    // Remove trailing slash and .git suffix
    let path = path.trim_end_matches('/').trim_end_matches(".git");

    let segments: Vec<&str> = path.split('/').collect();

    if segments.len() < 2 {
        return Err(BendclawError::Cli(format!(
            "invalid GitHub URL: \"{url}\" — expected at least owner/repo"
        )));
    }

    let repo = format!("{}/{}", segments[0], segments[1]);

    // https://github.com/owner/repo
    if segments.len() == 2 {
        return Ok(GitHubSource {
            repo,
            git_ref: None,
            subpath: None,
        });
    }

    // https://github.com/owner/repo/tree/<ref>/<path...>
    if segments.len() >= 4 && segments[2] == "tree" {
        let git_ref = segments[3].to_string();
        let subpath = if segments.len() > 4 {
            Some(PathBuf::from(segments[4..].join("/")))
        } else {
            None
        };
        return Ok(GitHubSource {
            repo,
            git_ref: Some(git_ref),
            subpath,
        });
    }

    // Reject unknown URL structures (e.g. /blob/..., /commit/..., etc.)
    Err(BendclawError::Cli(format!(
        "unsupported GitHub URL: \"{url}\"\n  only /tree/<ref>/<path> URLs are supported; use owner/repo to install the whole repository"
    )))
}

fn repo_name(repo: &str) -> String {
    repo.split('/').next_back().unwrap_or(repo).to_string()
}

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

fn check_gh_available() -> Result<()> {
    let result = Command::new("gh").arg("--version").output();
    match result {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err(BendclawError::Cli(
            "gh command not found; install GitHub CLI first: https://cli.github.com".into(),
        )),
    }
}

fn clone_repo(repo: &str, git_ref: Option<&str>, target: &Path) -> Result<()> {
    let mut cmd = Command::new("gh");
    cmd.arg("repo").arg("clone").arg(repo).arg(target);

    if let Some(r) = git_ref {
        cmd.arg("--").arg("--branch").arg(r);
    }

    let output = cmd
        .output()
        .map_err(|e| BendclawError::Cli(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        if msg.contains("Could not resolve") || msg.contains("not found") {
            return Err(BendclawError::Cli(format!("repository not found: {repo}")));
        }
        return Err(BendclawError::Cli(format!("gh repo clone failed: {msg}")));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Frontmatter parsing (lightweight, self-contained)
// ---------------------------------------------------------------------------

fn read_skill_description(skill_md: &Path) -> Option<String> {
    let content = fs::read_to_string(skill_md).ok()?;
    let fm = extract_frontmatter(&content)?;
    for line in fm.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("description:") {
            let val = unquote(rest.trim());
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

pub struct SkillVariable {
    pub name: String,
    pub description: String,
}

/// Extract the YAML frontmatter block (between `---` markers).
pub fn extract_frontmatter(content: &str) -> Option<&str> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_open = &trimmed[3..];
    let end = after_open.find("\n---")?;
    Some(&after_open[..end])
}

/// Parse variables from YAML frontmatter.
///
/// Expects format:
/// ```yaml
/// variables:
///   - name: FOO
///     description: some desc
///     required: true
/// ```
pub fn parse_variables_from_frontmatter(fm: &str) -> Vec<SkillVariable> {
    let mut vars = Vec::new();
    let mut in_variables = false;
    let mut current_name: Option<String> = None;
    let mut current_desc = String::new();

    for line in fm.lines() {
        let trimmed = line.trim();

        if trimmed == "variables:" || trimmed.starts_with("variables:") {
            in_variables = true;
            continue;
        }

        if !in_variables {
            continue;
        }

        // End of variables block: a non-indented line that isn't a list item
        if !line.starts_with(' ') && !line.starts_with('\t') {
            break;
        }

        let stripped = trimmed.trim_start_matches('-').trim();

        if let Some(rest) = stripped.strip_prefix("name:") {
            // Flush previous
            if let Some(name) = current_name.take() {
                vars.push(SkillVariable {
                    name,
                    description: std::mem::take(&mut current_desc),
                });
            }
            current_name = Some(unquote(rest.trim()));
        } else if let Some(rest) = stripped.strip_prefix("description:") {
            current_desc = unquote(rest.trim());
        }
        // Ignore other fields like `required:`
    }

    // Flush last
    if let Some(name) = current_name {
        vars.push(SkillVariable {
            name,
            description: current_desc,
        });
    }

    vars
}

fn unquote(s: &str) -> String {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}
