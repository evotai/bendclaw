//! Memory store — manages two-layer memory directories (global + project).
//!
//! Each memory entry is a markdown file with YAML frontmatter:
//!
//! ```text
//! ---
//! name: feedback_plan_language
//! description: Plans must be written in Chinese
//! type: feedback
//! ---
//!
//! The user requires all plans to be written in Chinese.
//! ```
//!
//! `MEMORY.md` in each directory is an auto-generated index — never edited
//! directly. It is rebuilt from the topic files after every mutation.

use std::fmt;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Which memory layer to operate on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryScope {
    Global,
    Project,
}

impl MemoryScope {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "global" => Ok(Self::Global),
            "project" => Ok(Self::Project),
            other => Err(format!(
                "Unknown scope '{other}'. Use 'global' or 'project'."
            )),
        }
    }
}

impl fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => write!(f, "global"),
            Self::Project => write!(f, "project"),
        }
    }
}

/// Category of a memory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryKind {
    User,
    Feedback,
    Project,
    Reference,
}

impl MemoryKind {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "user" => Ok(Self::User),
            "feedback" => Ok(Self::Feedback),
            "project" => Ok(Self::Project),
            "reference" => Ok(Self::Reference),
            other => Err(format!(
                "Unknown type '{other}'. Use: user, feedback, project, reference."
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Feedback => "feedback",
            Self::Project => "project",
            Self::Reference => "reference",
        }
    }
}

/// Metadata parsed from a memory file's frontmatter.
#[derive(Debug, Clone)]
pub struct EntryMeta {
    pub name: String,
    pub description: String,
    pub kind: MemoryKind,
    pub bytes: usize,
}

// ---------------------------------------------------------------------------
// Quota constants
// ---------------------------------------------------------------------------

const MAX_SCOPE_BYTES: usize = 25_000;
const MAX_FILE_BYTES: usize = 5_000;
const MAX_ENTRIES: usize = 50;
const INDEX_FILE: &str = "MEMORY.md";

// ---------------------------------------------------------------------------
// MemoryStore
// ---------------------------------------------------------------------------

pub struct MemoryStore {
    global_dir: PathBuf,
    project_dir: PathBuf,
}

impl MemoryStore {
    pub fn new(global_dir: PathBuf, project_dir: PathBuf) -> Self {
        Self {
            global_dir,
            project_dir,
        }
    }

    // -- public API ----------------------------------------------------------

    /// List entries or read a single entry.
    pub fn read(&self, scope: MemoryScope, name: Option<&str>) -> Result<String, String> {
        let dir = self.dir_for(scope);
        ensure_dir(dir);

        match name {
            Some(n) => {
                validate_name(n)?;
                self.read_single(dir, n)
            }
            None => self.read_list(scope, dir),
        }
    }

    /// Create a new entry. Fails if the name already exists.
    pub fn add(
        &self,
        scope: MemoryScope,
        name: &str,
        description: &str,
        kind: MemoryKind,
        body: &str,
    ) -> Result<String, String> {
        validate_name(name)?;
        validate_description(description)?;

        let dir = self.dir_for(scope);
        ensure_dir(dir);

        let path = entry_path(dir, name);
        if path.exists() {
            return Err(format!(
                "Memory '{name}' already exists in {scope} scope. Use replace to update it."
            ));
        }

        let content = build_file_content(name, description, kind, body);
        self.check_quota(scope, content.len(), None)?;
        fs::write(&path, &content).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
        self.rebuild_index(scope)?;
        self.format_response(scope, &format!("Added '{name}' to {scope} memory."))
    }

    /// Overwrite an existing entry. Fails if the name does not exist.
    pub fn replace(
        &self,
        scope: MemoryScope,
        name: &str,
        description: &str,
        kind: MemoryKind,
        body: &str,
    ) -> Result<String, String> {
        validate_name(name)?;
        validate_description(description)?;

        let dir = self.dir_for(scope);
        let path = entry_path(dir, name);
        if !path.exists() {
            return Err(format!(
                "Memory '{name}' not found in {scope} scope. Use add to create it."
            ));
        }

        let old_bytes = fs::metadata(&path).map(|m| m.len() as usize).unwrap_or(0);
        let content = build_file_content(name, description, kind, body);
        self.check_quota(scope, content.len(), Some(old_bytes))?;
        fs::write(&path, &content).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
        self.rebuild_index(scope)?;
        self.format_response(scope, &format!("Updated '{name}' in {scope} memory."))
    }

    /// Delete an entry. Fails if the name does not exist.
    pub fn remove(&self, scope: MemoryScope, name: &str) -> Result<String, String> {
        validate_name(name)?;

        let dir = self.dir_for(scope);
        let path = entry_path(dir, name);
        if !path.exists() {
            return Err(format!("Memory '{name}' not found in {scope} scope."));
        }

        fs::remove_file(&path).map_err(|e| format!("Cannot remove {}: {e}", path.display()))?;
        self.rebuild_index(scope)?;
        self.format_response(scope, &format!("Removed '{name}' from {scope} memory."))
    }

    // -- internals -----------------------------------------------------------

    fn dir_for(&self, scope: MemoryScope) -> &Path {
        match scope {
            MemoryScope::Global => &self.global_dir,
            MemoryScope::Project => &self.project_dir,
        }
    }

    fn read_single(&self, dir: &Path, name: &str) -> Result<String, String> {
        let path = entry_path(dir, name);
        if !path.exists() {
            return Err(format!("Memory '{name}' not found."));
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        Ok(content)
    }

    fn read_list(&self, scope: MemoryScope, dir: &Path) -> Result<String, String> {
        let entries = list_entries(dir)?;
        let total_bytes: usize = entries.iter().map(|e| e.bytes).sum();

        let mut out = String::new();
        if entries.is_empty() {
            out.push_str(&format!("No memories in {scope} scope.\n"));
        } else {
            for entry in &entries {
                out.push_str(&format!(
                    "- {} [{}] ({} bytes): {}\n",
                    entry.name,
                    entry.kind.as_str(),
                    entry.bytes,
                    entry.description,
                ));
            }
        }
        out.push_str(&format!(
            "\nUsage: {total_bytes} / {MAX_SCOPE_BYTES} bytes ({:.0}%), {} / {MAX_ENTRIES} entries",
            (total_bytes as f64 / MAX_SCOPE_BYTES as f64) * 100.0,
            entries.len(),
        ));
        Ok(out)
    }

    fn check_quota(
        &self,
        scope: MemoryScope,
        new_bytes: usize,
        replacing_bytes: Option<usize>,
    ) -> Result<(), String> {
        if new_bytes > MAX_FILE_BYTES {
            return Err(format!(
                "Entry too large: {new_bytes} bytes (limit: {MAX_FILE_BYTES}). \
                 Keep memory entries concise."
            ));
        }

        let dir = self.dir_for(scope);
        let entries = list_entries(dir).unwrap_or_default();
        let current_total: usize = entries.iter().map(|e| e.bytes).sum();
        let adjusted_total = current_total - replacing_bytes.unwrap_or(0);

        if adjusted_total + new_bytes > MAX_SCOPE_BYTES {
            return Err(format!(
                "Quota exceeded: adding {new_bytes} bytes would bring {scope} scope to {} / {MAX_SCOPE_BYTES} bytes. \
                 Remove or shorten existing entries first.",
                adjusted_total + new_bytes,
            ));
        }

        if replacing_bytes.is_none() && entries.len() >= MAX_ENTRIES {
            return Err(format!(
                "Entry limit reached: {scope} scope has {MAX_ENTRIES} entries. \
                 Remove an entry before adding a new one.",
            ));
        }

        Ok(())
    }

    fn rebuild_index(&self, scope: MemoryScope) -> Result<(), String> {
        let dir = self.dir_for(scope);
        let mut entries = list_entries(dir)?;

        // Sort by kind (user, feedback, project, reference), then by name
        entries.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));

        let mut index = String::new();
        for entry in &entries {
            index.push_str(&format!(
                "- [{}]({}.md) — {}\n",
                entry.name, entry.name, entry.description,
            ));
        }

        let index_path = dir.join(INDEX_FILE);
        fs::write(&index_path, &index)
            .map_err(|e| format!("Cannot write index {}: {e}", index_path.display()))?;
        Ok(())
    }

    fn format_response(&self, scope: MemoryScope, message: &str) -> Result<String, String> {
        let dir = self.dir_for(scope);
        let entries = list_entries(dir)?;
        let total_bytes: usize = entries.iter().map(|e| e.bytes).sum();

        let mut out = format!("{message}\n\nCurrent entries:\n");
        for entry in &entries {
            out.push_str(&format!(
                "- {} [{}]: {}\n",
                entry.name,
                entry.kind.as_str(),
                entry.description,
            ));
        }
        out.push_str(&format!(
            "\nUsage: {total_bytes} / {MAX_SCOPE_BYTES} bytes ({:.0}%), {} / {MAX_ENTRIES} entries",
            (total_bytes as f64 / MAX_SCOPE_BYTES as f64) * 100.0,
            entries.len(),
        ));
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validate that a memory entry name is a safe filename slug.
/// Allows only ASCII alphanumeric, underscore, and hyphen. Must be non-empty
/// and at most 100 characters.
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Memory name must not be empty.".into());
    }
    if name.len() > 100 {
        return Err(format!(
            "Memory name too long: {} chars (limit: 100).",
            name.len()
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "Invalid memory name '{name}'. \
             Only ASCII letters, digits, underscore, and hyphen are allowed."
        ));
    }
    Ok(())
}

/// Validate that a description is a single line without frontmatter delimiters.
fn validate_description(desc: &str) -> Result<(), String> {
    if desc.is_empty() {
        return Err("Description must not be empty.".into());
    }
    if desc.len() > 200 {
        return Err(format!(
            "Description too long: {} chars (limit: 200).",
            desc.len()
        ));
    }
    if desc.contains('\n') || desc.contains('\r') {
        return Err("Description must be a single line.".into());
    }
    if desc.contains("---") {
        return Err("Description must not contain '---' (frontmatter delimiter).".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// File helpers
// ---------------------------------------------------------------------------

fn entry_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.md"))
}

fn ensure_dir(dir: &Path) {
    let _ = fs::create_dir_all(dir);
}

fn build_file_content(name: &str, description: &str, kind: MemoryKind, body: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: {description}\ntype: {}\n---\n\n{body}\n",
        kind.as_str(),
    )
}

/// Scan a directory for memory entry files and parse their frontmatter.
fn list_entries(dir: &Path) -> Result<Vec<EntryMeta>, String> {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(Vec::new()),
    };

    let mut entries = Vec::new();
    for item in read_dir {
        let item = match item {
            Ok(i) => i,
            Err(_) => continue,
        };
        let path = item.path();
        if !path.is_file() {
            continue;
        }
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Skip the index file itself
        if file_name == INDEX_FILE {
            continue;
        }
        // Only process .md files
        if !file_name.ends_with(".md") {
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let bytes = content.len();

        if let Some(meta) = parse_frontmatter(&content, bytes) {
            entries.push(meta);
        }
    }

    Ok(entries)
}

/// Parse YAML frontmatter from a memory file.
/// Returns `None` if frontmatter is missing or invalid.
fn parse_frontmatter(content: &str, bytes: usize) -> Option<EntryMeta> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_open = &trimmed[3..];
    let end = after_open.find("\n---")?;
    let yaml_block = &after_open[..end];

    let mut name = None;
    let mut description = None;
    let mut kind_str = None;

    for line in yaml_block.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name:") {
            name = Some(unquote(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("description:") {
            description = Some(unquote(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("type:") {
            kind_str = Some(unquote(rest.trim()));
        }
    }

    let name = name?;
    let description = description.unwrap_or_default();
    let kind = kind_str
        .and_then(|s| MemoryKind::parse(&s).ok())
        .unwrap_or(MemoryKind::Reference);

    Some(EntryMeta {
        name,
        description,
        kind,
        bytes,
    })
}

fn unquote(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}
