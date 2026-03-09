use crate::kernel::skills::catalog::catalog_cache::CatalogCache;
use crate::kernel::skills::fs::SkillMirror;

pub(super) fn load_builtins(mirror: &SkillMirror, cache: &CatalogCache) {
    for loaded in mirror.load_builtins() {
        cache.insert(loaded.skill.name.clone(), loaded);
    }
}

pub(super) fn log_loaded_skills(cache: &CatalogCache) {
    let loaded = cache.all_loaded();
    for ls in &loaded {
        let skill = &ls.skill;
        let prompt_tokens = crate::llm::tokens::count_tokens(&skill.content);

        tracing::info!(
            "loading skill \"{}\"({}) v{} executable={} params={}",
            skill.name,
            ls.fs_dir.display(),
            skill.version,
            skill.executable,
            skill.parameters.len(),
        );

        let mut ref_files = Vec::new();
        collect_ref_md_files(&ls.fs_dir, &ls.fs_dir, &mut ref_files);

        let total_entries = 1 + ref_files.len();
        let prefix = if total_entries == 1 {
            "\u{2514}\u{2500}\u{2500}"
        } else {
            "\u{251c}\u{2500}\u{2500}"
        };
        tracing::info!("  {} SKILL.md ({} tokens) [loaded]", prefix, prompt_tokens);

        let on_demand_tokens: usize = ref_files.iter().map(|(_, t)| *t).sum();
        for (i, (rel_path, tokens)) in ref_files.iter().enumerate() {
            let prefix = if i + 1 == ref_files.len() {
                "\u{2514}\u{2500}\u{2500}"
            } else {
                "\u{251c}\u{2500}\u{2500}"
            };
            tracing::info!("  {} {} ({} tokens) [on-demand]", prefix, rel_path, tokens);
        }

        tracing::info!(
            "  skill \"{}\": loaded={} tokens, on_demand={} files / {} tokens",
            skill.name,
            prompt_tokens,
            ref_files.len(),
            on_demand_tokens,
        );
    }
}

fn collect_ref_md_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<(String, usize)>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut children: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    children.sort();
    for path in children {
        if path.is_dir() {
            collect_ref_md_files(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let rel = path
                .strip_prefix(root)
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_default();
            if rel == "SKILL.md" {
                continue;
            }
            let tokens = match std::fs::read_to_string(&path) {
                Ok(content) => crate::llm::tokens::count_tokens(&content),
                Err(_) => 0,
            };
            out.push((rel, tokens));
        }
    }
}
