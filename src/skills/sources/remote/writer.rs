//! Write/remove skill files to per-user remote directories.
//!
//! Writes are atomic: content goes to a temp directory first, then a rename
//! swaps it into place.  This prevents readers from seeing half-written state.

use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

use super::paths;
use crate::skills::definition::manifest::SkillManifest;
use crate::skills::definition::skill::Skill;
use crate::skills::definition::skill::SkillParameter;
use crate::skills::definition::skill::SkillRequirements;
use crate::skills::diagnostics;
use crate::skills::fs::is_safe_relative_path;
use crate::skills::fs::load_skill_from_dir;
use crate::skills::fs::load_skill_with_meta;
use crate::skills::fs::LoadedSkill;

/// Metadata persisted alongside SKILL.md so that scope, source, user_id,
/// etc. survive a round-trip through the filesystem mirror.
#[derive(Serialize, Deserialize)]
pub struct SkillMeta {
    pub scope: String,
    pub source: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub created_by: Option<String>,
    pub executable: bool,
    #[serde(default)]
    pub parameters: Vec<SkillParameter>,
    #[serde(default)]
    pub requires: Option<SkillRequirements>,
    #[serde(default)]
    pub manifest: Option<SkillManifest>,
}

/// Write a skill to the local mirror atomically.
pub fn write_skill(workspace_root: &Path, user_id: &str, skill: &Skill) -> Option<LoadedSkill> {
    let final_dir = paths::skill_dir(workspace_root, user_id, &skill.name);
    let parent = final_dir.parent()?;
    std::fs::create_dir_all(parent).ok()?;

    // Stage into a temp directory next to the final location
    let tmp_dir = parent.join(format!(".tmp-{}", skill.name));
    if tmp_dir.exists() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
    std::fs::create_dir_all(&tmp_dir).ok()?;

    // Write SKILL.md
    let skill_md = format!(
        "---\nname: {}\ndescription: {}\nversion: {}\ntimeout: {}\n---\n{}",
        skill.name, skill.description, skill.version, skill.timeout, skill.content
    );
    if std::fs::write(tmp_dir.join("SKILL.md"), &skill_md).is_err() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return None;
    }

    // Write .meta.json — preserves fields not in SKILL.md frontmatter
    let meta = SkillMeta {
        scope: skill.scope.as_str().to_string(),
        source: skill.source.as_str().to_string(),
        user_id: skill.user_id.clone(),
        created_by: skill.created_by.clone(),
        executable: skill.executable,
        parameters: skill.parameters.clone(),
        requires: skill.requires.clone(),
        manifest: skill.manifest.clone(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(tmp_dir.join(".meta.json"), json);
    }

    // Write files (scripts/, references/) — skip SKILL.md since it's written above with frontmatter.
    for f in &skill.files {
        if f.path == "SKILL.md" {
            continue;
        }
        let rel = std::path::Path::new(&f.path);
        if !is_safe_relative_path(rel) {
            diagnostics::log_skill_unsafe_path(&skill.name, &f.path);
            continue;
        }
        let file_path = tmp_dir.join(rel);
        if let Some(p) = file_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(&file_path, &f.body);
    }

    // Atomic swap: remove old dir, rename tmp into place
    if final_dir.exists() {
        let _ = std::fs::remove_dir_all(&final_dir);
    }
    if std::fs::rename(&tmp_dir, &final_dir).is_err() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return None;
    }

    let mut loaded = load_skill_from_dir(&final_dir, &skill.name)?;
    loaded.skill.scope = skill.scope.clone();
    loaded.skill.source = skill.source.clone();
    loaded.skill.user_id = skill.user_id.clone();
    loaded.skill.created_by = skill.created_by.clone();
    loaded.skill.executable = skill.executable;
    loaded.skill.parameters = skill.parameters.clone();
    loaded.skill.files = skill.files.clone();
    loaded.skill.requires = skill.requires.clone();
    loaded.skill.manifest = skill.manifest.clone();
    Some(loaded)
}

/// Compute the current on-disk checksum, including mirror metadata.
pub fn read_disk_checksum(
    workspace_root: &Path,
    user_id: &str,
    skill_name: &str,
) -> Option<String> {
    let dir = paths::skill_dir(workspace_root, user_id, skill_name);
    let loaded = load_skill_with_meta(&dir, skill_name)?;
    Some(loaded.skill.compute_sha256())
}

pub fn remove_skill(workspace_root: &Path, user_id: &str, skill_name: &str) {
    let dir = paths::skill_dir(workspace_root, user_id, skill_name);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Write a subscribed skill to the local mirror under `.remote/subscribed/{owner_id}/{skill_name}/`.
pub fn write_subscribed_skill(
    workspace_root: &Path,
    subscriber_id: &str,
    owner_id: &str,
    skill: &Skill,
) -> Option<LoadedSkill> {
    let final_dir =
        paths::subscribed_skill_dir(workspace_root, subscriber_id, owner_id, &skill.name);
    let parent = final_dir.parent()?;
    std::fs::create_dir_all(parent).ok()?;

    let tmp_dir = parent.join(format!(".tmp-{}", skill.name));
    if tmp_dir.exists() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
    std::fs::create_dir_all(&tmp_dir).ok()?;

    let skill_md = format!(
        "---\nname: {}\ndescription: {}\nversion: {}\ntimeout: {}\n---\n{}",
        skill.name, skill.description, skill.version, skill.timeout, skill.content
    );
    if std::fs::write(tmp_dir.join("SKILL.md"), &skill_md).is_err() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return None;
    }

    let meta = SkillMeta {
        scope: skill.scope.as_str().to_string(),
        source: skill.source.as_str().to_string(),
        user_id: skill.user_id.clone(),
        created_by: skill.created_by.clone(),
        executable: skill.executable,
        parameters: skill.parameters.clone(),
        requires: skill.requires.clone(),
        manifest: skill.manifest.clone(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(tmp_dir.join(".meta.json"), json);
    }

    for f in &skill.files {
        if f.path == "SKILL.md" {
            continue;
        }
        let rel = std::path::Path::new(&f.path);
        if !is_safe_relative_path(rel) {
            diagnostics::log_skill_unsafe_path(&skill.name, &f.path);
            continue;
        }
        let file_path = tmp_dir.join(rel);
        if let Some(p) = file_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(&file_path, &f.body);
    }

    if final_dir.exists() {
        let _ = std::fs::remove_dir_all(&final_dir);
    }
    if std::fs::rename(&tmp_dir, &final_dir).is_err() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return None;
    }

    let mut loaded = load_skill_from_dir(&final_dir, &skill.name)?;
    loaded.skill.scope = skill.scope.clone();
    loaded.skill.source = skill.source.clone();
    loaded.skill.user_id = skill.user_id.clone();
    loaded.skill.created_by = skill.created_by.clone();
    loaded.skill.executable = skill.executable;
    loaded.skill.parameters = skill.parameters.clone();
    loaded.skill.files = skill.files.clone();
    loaded.skill.requires = skill.requires.clone();
    loaded.skill.manifest = skill.manifest.clone();
    Some(loaded)
}

/// Compute the on-disk checksum for a subscribed skill mirror.
pub fn read_subscribed_disk_checksum(
    workspace_root: &Path,
    subscriber_id: &str,
    owner_id: &str,
    skill_name: &str,
) -> Option<String> {
    let dir = paths::subscribed_skill_dir(workspace_root, subscriber_id, owner_id, skill_name);
    let loaded = load_skill_with_meta(&dir, skill_name)?;
    Some(loaded.skill.compute_sha256())
}
