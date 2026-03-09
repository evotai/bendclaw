//! Filesystem operations: skill loading, disk mirror, and loaded skill wrapper.

use std::collections::HashMap;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillParameter;
use crate::kernel::skills::skill::SkillRequirements;
use crate::kernel::skills::skill::SkillScope;
use crate::kernel::skills::skill::SkillSource;

// ── LoadedSkill ───────────────────────────────────────────────────────────────

/// A skill loaded from the local filesystem.
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    pub skill: Skill,
    /// Absolute path to the skill's root directory.
    pub fs_dir: PathBuf,
}

impl LoadedSkill {
    pub fn script_path(&self) -> Option<PathBuf> {
        find_script(&self.fs_dir)
    }

    /// Read skill documentation by sub-path.
    pub fn read_doc(&self, sub_path: &str) -> Option<String> {
        if sub_path.is_empty() {
            return Some(self.skill.content.clone());
        }
        let target = safe_doc_target(&self.fs_dir, sub_path)?;
        if target.is_file() && target.extension().and_then(|e| e.to_str()) == Some("md") {
            return std::fs::read_to_string(&target).ok();
        }
        if target.is_dir() {
            let mut files: Vec<String> = std::fs::read_dir(&target)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().and_then(|e| e.to_str()) == Some("md") {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            if !files.is_empty() {
                files.sort();
                return Some(files.join("\n"));
            }
        }
        None
    }
}

// ── SkillMirror ───────────────────────────────────────────────────────────────

/// Manages the two-layer filesystem layout: builtins (read-only) + remote (writable).
pub struct SkillMirror {
    pub builtins_dir: PathBuf,
    pub remote_dir: PathBuf,
}

impl SkillMirror {
    pub fn new(local_dir: PathBuf) -> Self {
        let builtins_dir = local_dir.clone();
        let remote_dir = local_dir.join(".remote");
        if let Err(e) = std::fs::create_dir_all(&builtins_dir) {
            tracing::warn!(error = %e, path = %builtins_dir.display(), "failed to create builtins dir");
        }
        if let Err(e) = std::fs::create_dir_all(&remote_dir) {
            tracing::warn!(error = %e, path = %remote_dir.display(), "failed to create remote dir");
        }
        Self {
            builtins_dir,
            remote_dir,
        }
    }

    pub fn load_builtins(&self) -> Vec<LoadedSkill> {
        load_skills(&self.builtins_dir)
    }

    pub fn skill_dir(&self, skill: &Skill) -> PathBuf {
        if skill.source == SkillSource::Local {
            self.builtins_dir.join(&skill.name)
        } else {
            self.remote_dir.join(&skill.name)
        }
    }

    pub fn write_remote_skill(&self, name: &str, skill: &Skill) -> Option<LoadedSkill> {
        let skill_dir = self.remote_dir.join(name);
        self.write_skill_to_dir(&skill_dir, name, skill)
    }

    pub fn write_skill(&self, skill: &Skill) -> Option<LoadedSkill> {
        let skill_dir = self.skill_dir(skill);
        self.write_skill_to_dir(&skill_dir, &skill.name, skill)
    }

    fn write_skill_to_dir(
        &self,
        skill_dir: &Path,
        fallback_name: &str,
        skill: &Skill,
    ) -> Option<LoadedSkill> {
        if skill_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(skill_dir) {
                tracing::warn!(skill = %skill.name, error = %e, "failed to clean skill dir on insert");
                return None;
            }
        }
        if let Err(e) = std::fs::create_dir_all(skill_dir) {
            tracing::warn!(skill = %skill.name, error = %e, "failed to create skill dir on insert");
            return None;
        }

        let skill_md = format!(
            "---\nname: {}\ndescription: {}\nversion: {}\ntimeout: {}\n---\n{}",
            skill.name, skill.description, skill.version, skill.timeout, skill.content
        );
        if let Err(e) = std::fs::write(skill_dir.join("SKILL.md"), &skill_md) {
            tracing::warn!(skill = %skill.name, error = %e, "failed to write SKILL.md on insert");
            return None;
        }

        for f in &skill.files {
            let rel = Path::new(&f.path);
            if !is_safe_relative_path(rel) {
                tracing::warn!(skill = %skill.name, path = %f.path, "unsafe skill file path rejected");
                continue;
            }
            let file_path = skill_dir.join(rel);
            if let Some(parent) = file_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    tracing::warn!(skill = %skill.name, path = %f.path, error = %e, "failed to create parent dir");
                    continue;
                }
            }
            if let Err(e) = std::fs::write(&file_path, &f.body) {
                tracing::warn!(skill = %skill.name, path = %f.path, error = %e, "failed to write skill file");
                continue;
            }
        }

        let mut loaded = load_skill_from_dir(skill_dir, fallback_name)?;
        loaded.skill.scope = skill.scope.clone();
        loaded.skill.source = skill.source.clone();
        loaded.skill.agent_id = skill.agent_id.clone();
        loaded.skill.user_id = skill.user_id.clone();
        loaded.skill.executable = skill.executable;
        loaded.skill.parameters = skill.parameters.clone();
        loaded.skill.files = skill.files.clone();
        loaded.skill.requires = skill.requires.clone();
        Some(loaded)
    }

    pub fn remove_skill(&self, name: &str) {
        if !is_safe_relative_path(Path::new(name)) {
            tracing::warn!(skill = %name, "unsafe skill name rejected for removal");
            return;
        }
        let dir = self.remote_dir.join(name);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                tracing::warn!(skill = %name, error = %e, "failed to remove skill dir on evict");
            }
        }
    }

    pub fn remove_remote_skill(&self, name: &str) {
        if !is_safe_relative_path(Path::new(name)) {
            tracing::warn!(skill = %name, "unsafe skill name rejected for stale removal");
            return;
        }
        let dir = self.remote_dir.join(name);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                tracing::warn!(skill = %name, error = %e, "failed to remove stale remote skill dir");
            }
        }
    }
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

// ── Loader ────────────────────────────────────────────────────────────────────

/// Load all skills from a directory.
pub fn load_skills(skills_dir: &Path) -> Vec<LoadedSkill> {
    let entries = match std::fs::read_dir(skills_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(dir = %skills_dir.display(), error = %e, "cannot read skills directory");
            return Vec::new();
        }
    };

    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if !n.starts_with('_') => n.to_string(),
            _ => continue,
        };
        if let Some(loaded) =
            load_versioned_skill(&path, &dir_name).or_else(|| load_skill_from_dir(&path, &dir_name))
        {
            skills.push(loaded);
        }
    }

    tracing::info!(count = skills.len(), "skills loaded");
    skills
}

fn load_versioned_skill(skill_dir: &Path, dir_name: &str) -> Option<LoadedSkill> {
    let mut versions: Vec<(semver::Version, PathBuf)> = std::fs::read_dir(skill_dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.is_dir() {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| semver::Version::parse(n).ok())
                    .map(|v| (v, p))
            } else {
                None
            }
        })
        .collect();

    if versions.is_empty() {
        return None;
    }
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    load_skill_from_dir(&versions[0].1, dir_name)
}

/// Load a skill from a directory that contains a `SKILL.md` file.
pub fn load_skill_from_dir(dir: &Path, fallback_name: &str) -> Option<LoadedSkill> {
    let content = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let (frontmatter, body) = parse_frontmatter(&content);

    let name = frontmatter
        .get("name")
        .cloned()
        .unwrap_or_else(|| fallback_name.to_string());
    let description = frontmatter.get("description").cloned().unwrap_or_default();
    let version = frontmatter
        .get("version")
        .cloned()
        .unwrap_or_else(|| "0.0.0".into());
    let timeout = frontmatter
        .get("timeout")
        .and_then(|t| t.parse().ok())
        .unwrap_or(30);

    let parameters = parse_parameters_section(&body);
    let executable = find_script(dir).is_some();
    let requires = parse_requires(&content);

    let skill = Skill {
        name,
        description,
        version,
        scope: SkillScope::Global,
        source: SkillSource::Local,
        agent_id: None,
        user_id: None,
        timeout,
        executable,
        parameters,
        content: body,
        files: Vec::new(),
        requires,
    };

    Some(LoadedSkill {
        skill,
        fs_dir: dir.to_path_buf(),
    })
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

pub fn parse_frontmatter(content: &str) -> (HashMap<String, String>, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (HashMap::new(), content.to_string());
    }
    let after = &trimmed[3..];
    if let Some(end) = after.find("---") {
        let yaml_block = &after[..end];
        let raw: HashMap<String, serde_yaml::Value> =
            serde_yaml::from_str(yaml_block).unwrap_or_default();
        let map = raw
            .into_iter()
            .filter_map(|(k, v)| match v {
                serde_yaml::Value::String(s) => Some((k, s)),
                serde_yaml::Value::Number(n) => Some((k, n.to_string())),
                serde_yaml::Value::Bool(b) => Some((k, b.to_string())),
                _ => None,
            })
            .collect();
        let body = after[end + 3..].trim_start().to_string();
        (map, body)
    } else {
        (HashMap::new(), content.to_string())
    }
}

fn extract_yaml_block(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after = &trimmed[3..];
    after.find("---").map(|end| after[..end].to_string())
}

#[derive(Deserialize, Default)]
struct FrontmatterWithRequires {
    #[serde(default)]
    requires: Option<SkillRequirements>,
}

fn parse_requires(content: &str) -> Option<SkillRequirements> {
    let yaml = extract_yaml_block(content)?;
    let parsed: FrontmatterWithRequires = serde_yaml::from_str(&yaml).ok()?;
    parsed.requires
}

pub fn parse_parameters_section(body: &str) -> Vec<SkillParameter> {
    let mut params = Vec::new();
    let mut in_params = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Parameters") {
            in_params = true;
        } else if in_params && trimmed.starts_with("## ") {
            break;
        } else if in_params && trimmed.starts_with("- `--") {
            if let Some(p) = parse_param_line(trimmed) {
                params.push(p);
            }
        }
    }
    params
}

pub fn parse_param_line(line: &str) -> Option<SkillParameter> {
    let rest = line.strip_prefix("- `--")?;
    let end = rest.find('`')?;
    let name = rest[..end].to_string();
    let after = rest[end + 1..].trim();
    let description = after.strip_prefix(':').unwrap_or(after).trim().to_string();
    let required = description.contains("(required)");
    Some(SkillParameter {
        name,
        description,
        param_type: "string".to_string(),
        required,
        default: None,
    })
}

fn find_script(dir: &Path) -> Option<PathBuf> {
    let scripts_dir = dir.join("scripts");
    if !scripts_dir.is_dir() {
        return None;
    }
    for name in &["run.py", "run.sh"] {
        let p = scripts_dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    std::fs::read_dir(&scripts_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e == "py" || e == "sh")
                .unwrap_or(false)
        })
}

fn safe_doc_target(root: &Path, sub_path: &str) -> Option<PathBuf> {
    let rel = Path::new(sub_path);
    if rel.is_absolute()
        || rel.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let target = root.join(rel).canonicalize().ok()?;
    if target.starts_with(&root) {
        Some(target)
    } else {
        None
    }
}
