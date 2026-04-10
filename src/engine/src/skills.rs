//! Skills — load AgentSkills-compatible skill directories and inject into system prompts.
//!
//! Follows the [AgentSkills](https://agentskills.io) open standard.
//! Skills are directories containing a `SKILL.md` file with YAML frontmatter.
//!
//! # Progressive Disclosure
//!
//! 1. **Metadata** (~100 tokens/skill) — name + description, always in the system prompt
//! 2. **Instructions** (<5k tokens) — SKILL.md body, loaded by the agent when activated
//! 3. **Resources** (unlimited) — scripts/, references/, assets/, loaded on demand
//!
//! The agent decides when to activate a skill based on the description. No trigger
//! engine needed — the LLM is smart enough.
//!
//! # Example
//!
//! ```rust,no_run
//! use bendengine::skills::SkillSet;
//!
//! let skills = SkillSet::load(&["./skills", "~/.yoagent/skills"]).unwrap();
//! println!("{}", skills.format_for_prompt());
//! // Inject into system prompt via Agent::with_skills()
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// A loaded skill with its metadata.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Skill name (must match directory name, lowercase + hyphens)
    pub name: String,
    /// Description of what the skill does and when to use it
    pub description: String,
    /// Absolute path to SKILL.md
    pub file_path: PathBuf,
    /// Absolute path to the skill directory
    pub base_dir: PathBuf,
    /// Where this skill was loaded from (e.g. "workspace", "global", or a custom label)
    pub source: String,
}

/// A collection of loaded skills.
#[derive(Debug, Clone, Default)]
pub struct SkillSet {
    skills: Vec<Skill>,
}

/// Errors during skill loading.
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("IO error reading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("SKILL.md in {path} missing required frontmatter field: {field}")]
    MissingField { path: PathBuf, field: &'static str },
    #[error("SKILL.md in {path} has invalid frontmatter: {detail}")]
    InvalidFrontmatter { path: PathBuf, detail: String },
}

impl SkillSet {
    /// Load skills from multiple directories. Later directories take precedence
    /// (skills with the same name from later dirs override earlier ones).
    pub fn load(dirs: &[impl AsRef<Path>]) -> Result<Self, SkillError> {
        let mut by_name: HashMap<String, Skill> = HashMap::new();

        for (i, dir) in dirs.iter().enumerate() {
            let dir = dir.as_ref();
            if !dir.exists() {
                continue;
            }
            let source = format!("dir:{}", i);
            let skills = load_skills_from_dir(dir, &source)?;
            for skill in skills {
                by_name.insert(skill.name.clone(), skill);
            }
        }

        let mut skills: Vec<Skill> = by_name.into_values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { skills })
    }

    /// Load skills from a single directory with a custom source label.
    pub fn load_dir(dir: impl AsRef<Path>, source: &str) -> Result<Self, SkillError> {
        let skills = load_skills_from_dir(dir.as_ref(), source)?;
        Ok(Self { skills })
    }

    /// Create an empty skill set.
    pub fn empty() -> Self {
        Self { skills: Vec::new() }
    }

    /// Merge another skill set into this one. Other's skills override on name conflict.
    pub fn merge(&mut self, other: SkillSet) {
        let mut by_name: HashMap<String, Skill> =
            self.skills.drain(..).map(|s| (s.name.clone(), s)).collect();
        for skill in other.skills {
            by_name.insert(skill.name.clone(), skill);
        }
        self.skills = by_name.into_values().collect();
        self.skills.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Get all loaded skills.
    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// Number of loaded skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether no skills are loaded.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Format skills for inclusion in a system prompt.
    ///
    /// Uses XML format per the [AgentSkills standard](https://agentskills.io/integrate-skills):
    /// ```xml
    /// <available_skills>
    ///   <skill>
    ///     <name>weather</name>
    ///     <description>Get current weather and forecasts.</description>
    ///     <location>/path/to/skills/weather/SKILL.md</location>
    ///   </skill>
    /// </available_skills>
    /// ```
    ///
    /// Returns an empty string if no skills are loaded.
    pub fn format_for_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut out = String::from(
            "When the user's request matches a skill's description, \
             use read_file to load the SKILL.md at the given location, \
             then follow its instructions to complete the task.\n\n",
        );
        out.push_str("<available_skills>\n");
        for skill in &self.skills {
            out.push_str("  <skill>\n");
            out.push_str(&format!("    <name>{}</name>\n", xml_escape(&skill.name)));
            out.push_str(&format!(
                "    <description>{}</description>\n",
                xml_escape(&skill.description)
            ));
            out.push_str(&format!(
                "    <location>{}</location>\n",
                xml_escape(&skill.file_path.to_string_lossy())
            ));
            out.push_str("  </skill>\n");
        }
        out.push_str("</available_skills>");
        out
    }
}

/// Scan a directory for skills. Looks for:
/// - `<dir>/<name>/SKILL.md` (standard layout)
fn load_skills_from_dir(dir: &Path, source: &str) -> Result<Vec<Skill>, SkillError> {
    let mut skills = Vec::new();

    let entries = fs::read_dir(dir).map_err(|e| SkillError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| SkillError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_md = path.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }

        let content = fs::read_to_string(&skill_md).map_err(|e| SkillError::Io {
            path: skill_md.clone(),
            source: e,
        })?;

        let (name, description) = parse_frontmatter(&content, &skill_md)?;

        // Validate name matches directory
        let dir_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Use directory name if frontmatter name doesn't match (be lenient)
        let name = if name == dir_name { name } else { dir_name };

        let base_dir = fs::canonicalize(&path).unwrap_or(path);
        let file_path = base_dir.join("SKILL.md");

        skills.push(Skill {
            name,
            description,
            file_path,
            base_dir,
            source: source.to_string(),
        });
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

/// Parse YAML frontmatter from SKILL.md content.
/// Expects `---\n...\n---` block at the start.
fn parse_frontmatter(content: &str, path: &Path) -> Result<(String, String), SkillError> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err(SkillError::InvalidFrontmatter {
            path: path.to_path_buf(),
            detail: "missing opening ---".into(),
        });
    }

    let after_open = &trimmed[3..];
    let end = after_open
        .find("\n---")
        .ok_or(SkillError::InvalidFrontmatter {
            path: path.to_path_buf(),
            detail: "missing closing ---".into(),
        })?;

    let yaml_block = &after_open[..end];

    let mut name = None;
    let mut description = None;

    for line in yaml_block.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name:") {
            name = Some(unquote(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("description:") {
            description = Some(unquote(rest.trim()));
        }
    }

    let name = name.ok_or(SkillError::MissingField {
        path: path.to_path_buf(),
        field: "name",
    })?;
    let description = description.ok_or(SkillError::MissingField {
        path: path.to_path_buf(),
        field: "description",
    })?;

    if name.is_empty() {
        return Err(SkillError::MissingField {
            path: path.to_path_buf(),
            field: "name",
        });
    }
    if description.is_empty() {
        return Err(SkillError::MissingField {
            path: path.to_path_buf(),
            field: "description",
        });
    }

    Ok((name, description))
}

/// Remove surrounding quotes from a YAML value.
fn unquote(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Minimal XML escaping for prompt generation.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn create_skill(dir: &Path, name: &str, description: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {}\ndescription: {}\n---\n\n# {}\n\nInstructions here.\n",
                name, description, name
            ),
        )
        .unwrap();
    }

    #[test]
    fn load_skills_from_directory() {
        let tmp = TempDir::new().unwrap();
        create_skill(tmp.path(), "weather", "Get current weather and forecasts.");
        create_skill(tmp.path(), "git", "Git operations: commit, branch, merge.");

        let skills = SkillSet::load(&[tmp.path()]).unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills.skills()[0].name, "git");
        assert_eq!(skills.skills()[1].name, "weather");
    }

    #[test]
    fn format_for_prompt_xml() {
        let tmp = TempDir::new().unwrap();
        create_skill(tmp.path(), "weather", "Get weather.");

        let skills = SkillSet::load(&[tmp.path()]).unwrap();
        let prompt = skills.format_for_prompt();

        assert!(prompt.contains("<available_skills>"));
        assert!(prompt.contains("<name>weather</name>"));
        assert!(prompt.contains("<description>Get weather.</description>"));
        assert!(prompt.contains("SKILL.md</location>"));
        assert!(prompt.contains("</available_skills>"));
    }

    #[test]
    fn empty_when_no_skills() {
        let tmp = TempDir::new().unwrap();
        let skills = SkillSet::load(&[tmp.path()]).unwrap();
        assert!(skills.is_empty());
        assert_eq!(skills.format_for_prompt(), "");
    }

    #[test]
    fn later_dirs_override_earlier() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        create_skill(dir1.path(), "weather", "Old description.");
        create_skill(dir2.path(), "weather", "New description.");

        let skills = SkillSet::load(&[dir1.path(), dir2.path()]).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills.skills()[0].description, "New description.");
    }

    #[test]
    fn skips_nonexistent_dirs() {
        let skills = SkillSet::load(&[Path::new("/nonexistent/path")]).unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn skips_dirs_without_skill_md() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("not-a-skill")).unwrap();
        fs::write(tmp.path().join("not-a-skill/README.md"), "hello").unwrap();

        let skills = SkillSet::load(&[tmp.path()]).unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn error_on_missing_frontmatter() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("bad-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# No frontmatter\n").unwrap();

        let result = SkillSet::load(&[tmp.path()]);
        assert!(result.is_err());
    }

    #[test]
    fn error_on_missing_name() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("no-name");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\ndescription: Has desc but no name.\n---\n",
        )
        .unwrap();

        let result = SkillSet::load(&[tmp.path()]);
        assert!(result.is_err());
    }

    #[test]
    fn error_on_missing_description() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("no-desc");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: no-desc\n---\n").unwrap();

        let result = SkillSet::load(&[tmp.path()]);
        assert!(result.is_err());
    }

    #[test]
    fn quoted_frontmatter_values() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("quoted");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: \"quoted\"\ndescription: 'A quoted description.'\n---\n",
        )
        .unwrap();

        let skills = SkillSet::load(&[tmp.path()]).unwrap();
        assert_eq!(skills.skills()[0].name, "quoted");
        assert_eq!(skills.skills()[0].description, "A quoted description.");
    }

    #[test]
    fn xml_escaping() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("escape-test");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: escape-test\ndescription: Uses <tags> & \"quotes\"\n---\n",
        )
        .unwrap();

        let skills = SkillSet::load(&[tmp.path()]).unwrap();
        let prompt = skills.format_for_prompt();
        assert!(prompt.contains("&lt;tags&gt;"));
        assert!(prompt.contains("&amp;"));
        assert!(prompt.contains("&quot;quotes&quot;"));
    }

    #[test]
    fn merge_skill_sets() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        create_skill(dir1.path(), "weather", "Weather v1.");
        create_skill(dir1.path(), "git", "Git operations.");
        create_skill(dir2.path(), "weather", "Weather v2.");
        create_skill(dir2.path(), "docker", "Docker management.");

        let mut set1 = SkillSet::load(&[dir1.path()]).unwrap();
        let set2 = SkillSet::load(&[dir2.path()]).unwrap();
        set1.merge(set2);

        assert_eq!(set1.len(), 3);
        let names: Vec<&str> = set1.skills().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["docker", "git", "weather"]);
        // weather should be v2 (merged override)
        assert_eq!(
            set1.skills()
                .iter()
                .find(|s| s.name == "weather")
                .unwrap()
                .description,
            "Weather v2."
        );
    }

    #[test]
    fn load_real_agentskills_format() {
        // Test with metadata field (should be ignored, we only parse name+description)
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("nano-banana-pro");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: nano-banana-pro
description: Generate or edit images via Gemini 3 Pro Image.
metadata:
  {
    "openclaw":
      {
        "emoji": "🍌",
        "requires": { "bins": ["uv"], "env": ["GEMINI_API_KEY"] },
      },
  }
---

# Nano Banana Pro

Use the bundled script to generate images.
"#,
        )
        .unwrap();

        let skills = SkillSet::load(&[tmp.path()]).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills.skills()[0].name, "nano-banana-pro");
        assert_eq!(
            skills.skills()[0].description,
            "Generate or edit images via Gemini 3 Pro Image."
        );
    }
}
