//! Filesystem operations: skill loading and loaded skill wrapper.

use std::collections::HashMap;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use crate::kernel::skills::diagnostics;
use crate::kernel::skills::model::manifest::SkillManifest;
use crate::kernel::skills::model::skill::Skill;
use crate::kernel::skills::model::skill::SkillFile;
use crate::kernel::skills::model::skill::SkillParameter;
use crate::kernel::skills::model::skill::SkillRequirements;
use crate::kernel::skills::model::skill::SkillScope;
use crate::kernel::skills::model::skill::SkillSource;
use crate::kernel::skills::sources::remote::writer::SkillMeta;

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

// ── Loader ────────────────────────────────────────────────────────────────────

/// Load all skills from a directory.
pub fn load_skills(skills_dir: &Path) -> Vec<LoadedSkill> {
    let entries = match std::fs::read_dir(skills_dir) {
        Ok(e) => e,
        Err(e) => {
            diagnostics::log_skill_dir_read_failed(skills_dir, &e);
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
            Some(n) if !n.starts_with('_') && !n.starts_with('.') => n.to_string(),
            _ => continue,
        };
        if let Some(loaded) = load_skill_tree(&path, &dir_name) {
            skills.push(loaded);
        }
    }

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
    load_skill_with_meta(&versions[0].1, dir_name)
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

    let manifest = std::fs::read_to_string(dir.join("manifest.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<SkillManifest>(&s).ok());
    let files = load_bundled_files(dir);

    let skill = Skill {
        name,
        description,
        version,
        scope: SkillScope::Shared,
        source: SkillSource::Local,
        user_id: String::new(),
        created_by: None,
        last_used_by: None,
        timeout,
        executable,
        parameters,
        content: body,
        files,
        requires,
        manifest,
    };

    Some(LoadedSkill {
        skill,
        fs_dir: dir.to_path_buf(),
    })
}

/// Load a skill from a directory, overlaying `.meta.json` if present.
/// This restores scope/source/user_id/executable that aren't in SKILL.md frontmatter.
pub fn load_skill_with_meta(dir: &Path, fallback_name: &str) -> Option<LoadedSkill> {
    let mut loaded = load_skill_from_dir(dir, fallback_name)?;
    let meta_path = dir.join(".meta.json");
    if let Ok(json) = std::fs::read_to_string(&meta_path) {
        if let Ok(meta) = serde_json::from_str::<SkillMeta>(&json) {
            loaded.skill.scope = SkillScope::parse(&meta.scope);
            loaded.skill.source = SkillSource::parse(&meta.source);
            loaded.skill.user_id = meta.user_id;
            loaded.skill.created_by = meta.created_by;
            loaded.skill.executable = meta.executable;
            loaded.skill.parameters = meta.parameters;
            loaded.skill.requires = meta.requires;
            loaded.skill.manifest = meta.manifest;
        }
    }
    Some(loaded)
}

/// Load a skill entry from a named directory, supporting both plain and
/// versioned layouts while consistently applying mirror metadata.
pub fn load_skill_tree(dir: &Path, fallback_name: &str) -> Option<LoadedSkill> {
    load_versioned_skill(dir, fallback_name).or_else(|| load_skill_with_meta(dir, fallback_name))
}

fn load_bundled_files(dir: &Path) -> Vec<SkillFile> {
    let mut files = Vec::new();
    for root_name in ["scripts", "references"] {
        let root = dir.join(root_name);
        if !root.is_dir() {
            continue;
        }
        collect_skill_files(dir, &root, &mut files);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn collect_skill_files(skill_root: &Path, dir: &Path, out: &mut Vec<SkillFile>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_skill_files(skill_root, &path, out);
            continue;
        }
        let rel = match path.strip_prefix(skill_root) {
            Ok(rel) if is_safe_relative_path(rel) => rel,
            _ => continue,
        };
        let rel_str = rel.to_string_lossy().to_string();
        if rel_str.starts_with("scripts/") || rel_str.starts_with("references/") {
            if let Ok(body) = std::fs::read_to_string(&path) {
                out.push(SkillFile {
                    path: rel_str,
                    body,
                });
            }
        }
    }
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

pub fn find_script(dir: &Path) -> Option<PathBuf> {
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
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
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

/// Check that a relative path has no `..` or absolute components.
pub fn is_safe_relative_path(path: &Path) -> bool {
    !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
}
