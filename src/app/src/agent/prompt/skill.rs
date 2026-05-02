use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use evot_engine::SkillSpec;

// ---------------------------------------------------------------------------
// Builtin skills — compiled into the binary via include_str!()
// ---------------------------------------------------------------------------

struct BuiltinDef {
    name: &'static str,
    content: &'static str,
}

const BUILTINS: &[BuiltinDef] = &[BuiltinDef {
    name: "review",
    content: include_str!("prompts/review.md"),
}];

/// Parse builtin skill definitions into `SkillSpec` values.
/// Returns specs with an empty `base_dir` (no filesystem path).
fn builtin_specs() -> Vec<SkillSpec> {
    let sentinel = Path::new("<builtin>");
    BUILTINS
        .iter()
        .filter_map(|def| {
            let description = parse_frontmatter(def.content, sentinel).ok()?;
            let instructions = strip_frontmatter(def.content).to_string();
            Some(SkillSpec {
                name: def.name.to_string(),
                description,
                instructions,
                base_dir: PathBuf::new(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SkillLoadError {
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

// ---------------------------------------------------------------------------
// Public loader — builtin first, then filesystem (same name overrides)
// ---------------------------------------------------------------------------

pub fn load_skills(dirs: &[impl AsRef<Path>]) -> Result<Vec<SkillSpec>, SkillLoadError> {
    // Start with builtins
    let mut by_name: HashMap<String, SkillSpec> = builtin_specs()
        .into_iter()
        .map(|s| (s.name.clone(), s))
        .collect();

    // Filesystem skills override builtins with the same name
    for dir in dirs {
        let dir = dir.as_ref();
        if !dir.exists() {
            continue;
        }
        let specs = load_skills_from_dir(dir)?;
        for spec in specs {
            by_name.insert(spec.name.clone(), spec);
        }
    }

    let mut specs: Vec<SkillSpec> = by_name.into_values().collect();
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(specs)
}

/// Load skills from filesystem directories only (no builtins).
pub fn load_fs_skills(dirs: &[impl AsRef<Path>]) -> Result<Vec<SkillSpec>, SkillLoadError> {
    let mut by_name: HashMap<String, SkillSpec> = HashMap::new();

    for dir in dirs {
        let dir = dir.as_ref();
        if !dir.exists() {
            continue;
        }
        let specs = load_skills_from_dir(dir)?;
        for spec in specs {
            by_name.insert(spec.name.clone(), spec);
        }
    }

    let mut specs: Vec<SkillSpec> = by_name.into_values().collect();
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(specs)
}

fn load_skills_from_dir(dir: &Path) -> Result<Vec<SkillSpec>, SkillLoadError> {
    let mut specs = Vec::new();

    let entries = fs::read_dir(dir).map_err(|e| SkillLoadError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| SkillLoadError::Io {
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

        let content = fs::read_to_string(&skill_md).map_err(|e| SkillLoadError::Io {
            path: skill_md.clone(),
            source: e,
        })?;

        let description = parse_frontmatter(&content, &skill_md)?;

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let base_dir = fs::canonicalize(&path).unwrap_or(path);
        let instructions = strip_frontmatter(&content).to_string();

        specs.push(SkillSpec {
            name,
            description,
            instructions,
            base_dir,
        });
    }

    specs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(specs)
}

fn parse_frontmatter(content: &str, path: &Path) -> Result<String, SkillLoadError> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err(SkillLoadError::InvalidFrontmatter {
            path: path.to_path_buf(),
            detail: "missing opening ---".into(),
        });
    }

    let after_open = &trimmed[3..];
    let end = after_open
        .find("\n---")
        .ok_or(SkillLoadError::InvalidFrontmatter {
            path: path.to_path_buf(),
            detail: "missing closing ---".into(),
        })?;

    let yaml_block = &after_open[..end];

    let mut description = None;

    for line in yaml_block.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("description:") {
            description = Some(unquote(rest.trim()));
        }
    }

    let description = description.ok_or(SkillLoadError::MissingField {
        path: path.to_path_buf(),
        field: "description",
    })?;

    if description.is_empty() {
        return Err(SkillLoadError::MissingField {
            path: path.to_path_buf(),
            field: "description",
        });
    }

    Ok(description)
}

fn unquote(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn strip_frontmatter(content: &str) -> &str {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content;
    }
    let after_open = &trimmed[3..];
    match after_open.find("\n---") {
        Some(end) => {
            let rest = &after_open[end + 4..];
            rest.strip_prefix('\n').unwrap_or(rest)
        }
        None => content,
    }
}
