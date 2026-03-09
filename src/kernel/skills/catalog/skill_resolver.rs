use std::path::PathBuf;

use crate::kernel::skills::catalog::catalog_cache::CatalogCache;
use crate::kernel::skills::fs::SkillMirror;
use crate::kernel::skills::skill::Skill;

pub(super) fn resolve(cache: &CatalogCache, tool_name: &str) -> Option<Skill> {
    let result = cache.resolve(tool_name).map(|s| s.skill);
    match &result {
        Some(s) => tracing::info!(
            tool_name,
            resolved_name = %s.name,
            executable = s.executable,
            "catalog.resolve: tool resolved to skill"
        ),
        None => tracing::info!(tool_name, "catalog.resolve: no skill found for tool"),
    }
    result
}

pub(super) fn script_path(
    cache: &CatalogCache,
    mirror: &SkillMirror,
    tool_name: &str,
) -> Option<String> {
    let loaded = cache.resolve(tool_name)?;
    let host_script = loaded.script_path()?;
    let rel = host_script.strip_prefix(&mirror.builtins_dir).ok()?;
    let result = format!("/workspace/skills/{}", rel.to_string_lossy());
    tracing::info!(tool_name, path = %result, "catalog.script_path: resolved");
    Some(result)
}

pub(super) fn host_script_path(cache: &CatalogCache, tool_name: &str) -> Option<PathBuf> {
    let result = cache.resolve(tool_name).and_then(|s| s.script_path());
    match &result {
        Some(p) => {
            tracing::info!(tool_name, path = %p.display(), "catalog.host_script_path: resolved")
        }
        None => tracing::info!(tool_name, "catalog.host_script_path: not found"),
    }
    result
}

pub(super) fn read_skill(cache: &CatalogCache, path: &str) -> Option<String> {
    tracing::info!(path, "catalog.read_skill: looking up skill doc");
    let (loaded, sub_path) = match cache.split_doc_path(path) {
        Some(v) => v,
        None => {
            tracing::info!(path, "catalog.read_skill: skill not found in cache");
            return None;
        }
    };
    tracing::info!(
        path,
        skill_name = %loaded.skill.name,
        sub_path = %sub_path,
        fs_dir = %loaded.fs_dir.display(),
        "catalog.read_skill: resolved skill, reading doc"
    );
    let result = loaded.read_doc(&sub_path);
    match &result {
        Some(content) => tracing::info!(
            path,
            skill_name = %loaded.skill.name,
            content_size = content.len(),
            content = %content,
            "catalog.read_skill: doc loaded"
        ),
        None => tracing::info!(
            path,
            skill_name = %loaded.skill.name,
            sub_path = %sub_path,
            "catalog.read_skill: doc file not found"
        ),
    }
    result
}
