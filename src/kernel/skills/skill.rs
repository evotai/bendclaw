//! Core skill domain types, validation, and schema generation.

use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::skills::manifest::SkillManifest;
use crate::kernel::skills::sanitizer::sanitize_skill_description;

// ── Enums ─────────────────────────────────────────────────────────────────────

/// Visibility scope of a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SkillScope {
    Private,
    #[default]
    Shared,
}

impl SkillScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Shared => "shared",
        }
    }

    /// Parse scope from string. Legacy `"agent"` / `"user"` map to `Private`,
    /// legacy `"global"` maps to `Shared`.
    pub fn parse(s: &str) -> Self {
        match s {
            "private" | "agent" | "user" => Self::Private,
            _ => Self::Shared,
        }
    }
}

impl fmt::Display for SkillScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Origin of a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    #[default]
    Local,
    Hub,
    Github,
    Agent,
}

impl SkillSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Hub => "hub",
            Self::Github => "github",
            Self::Agent => "agent",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "hub" => Self::Hub,
            "github" => Self::Github,
            "agent" => Self::Agent,
            _ => Self::Local,
        }
    }
}

impl fmt::Display for SkillSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Supporting types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillParameter {
    pub name: String,
    pub description: String,
    #[serde(rename = "type", default = "default_param_type")]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    pub default: Option<serde_json::Value>,
}

fn default_param_type() -> String {
    "string".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFile {
    pub path: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillRequirements {
    #[serde(default)]
    pub bins: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
}

fn default_timeout() -> u64 {
    30
}

// ── SkillId ──────────────────────────────────────────────────────────────────

/// Explicit composite primary key for a skill.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillId {
    pub owner_id: String,
    pub name: String,
}

impl fmt::Display for SkillId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner_id, self.name)
    }
}

// ── Skill ─────────────────────────────────────────────────────────────────────

/// The canonical skill domain model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub scope: SkillScope,
    #[serde(default)]
    pub source: SkillSource,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub last_used_by: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    pub executable: bool,
    #[serde(default)]
    pub parameters: Vec<SkillParameter>,
    pub content: String,
    #[serde(default)]
    pub files: Vec<SkillFile>,
    #[serde(default)]
    pub requires: Option<SkillRequirements>,
    #[serde(default)]
    pub manifest: Option<SkillManifest>,
}

impl Skill {
    pub fn skill_id(&self) -> SkillId {
        SkillId {
            owner_id: self.user_id.clone(),
            name: self.name.clone(),
        }
    }

    /// Check if this skill is visible to the given user.
    pub fn is_visible_to(&self, user_id: &str) -> bool {
        match self.scope {
            SkillScope::Shared => true,
            SkillScope::Private => self.user_id == user_id,
        }
    }

    /// SHA-256 over content + sorted file bodies. Used for sync diff.
    pub fn compute_sha256(&self) -> String {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(self.name.as_bytes());
        hasher.update(b"|");
        hasher.update(self.version.as_bytes());
        hasher.update(b"|");
        hasher.update(self.description.as_bytes());
        hasher.update(b"|");
        hasher.update(self.timeout.to_string().as_bytes());
        hasher.update(b"|");
        hasher.update(self.scope.as_str().as_bytes());
        hasher.update(b"|");
        hasher.update(self.source.as_str().as_bytes());
        hasher.update(b"|");
        hasher.update(self.user_id.as_bytes());
        hasher.update(b"|");
        hasher.update(self.created_by.as_deref().unwrap_or("").as_bytes());
        hasher.update(b"|");
        hasher.update(if self.executable { b"1" } else { b"0" });
        hasher.update(b"|");
        let parameters_json = serde_json::to_string(&self.parameters).unwrap_or_default();
        hasher.update(parameters_json.as_bytes());
        hasher.update(b"|");
        let requires_json = serde_json::to_string(&self.requires).unwrap_or_default();
        hasher.update(requires_json.as_bytes());
        hasher.update(b"|");
        let manifest_json = serde_json::to_string(&self.manifest).unwrap_or_default();
        hasher.update(manifest_json.as_bytes());
        hasher.update(b"|");
        hasher.update(self.content.as_bytes());
        let mut sorted: Vec<&SkillFile> = self.files.iter().collect();
        sorted.sort_by_key(|f| &f.path);
        for f in sorted {
            hasher.update(b"|");
            hasher.update(f.path.as_bytes());
            hasher.update(b"|");
            hasher.update(f.body.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    // ── JSON Schema ───────────────────────────────────────────────────────

    /// Build a JSON Schema object describing the parameters for LLM tool use.
    pub fn to_json_schema(&self) -> serde_json::Value {
        let sanitized = sanitize_skill_description(&self.description);

        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.parameters {
            properties.insert(
                param.name.clone(),
                json!({
                    "type": param.param_type,
                    "description": param.description,
                }),
            );
            if param.required {
                required.push(json!(param.name));
            }
        }

        json!({
            "type": "object",
            "description": sanitized.content,
            "properties": properties,
            "required": required,
        })
    }

    // ── Validation ────────────────────────────────────────────────────────

    /// Validate name, file paths, and content sizes.
    pub fn validate(&self) -> Result<()> {
        Self::validate_name(&self.name)?;
        for f in &self.files {
            Self::validate_file_path(&f.path)?;
        }
        Self::validate_size(&self.content, &self.files)
    }

    pub fn validate_name(name: &str) -> Result<()> {
        if name.len() < MIN_NAME_LEN || name.len() > MAX_NAME_LEN {
            return Err(ErrorCode::skill_validation(format!(
                "skill name must be {MIN_NAME_LEN}-{MAX_NAME_LEN} chars, got {}",
                name.len()
            )));
        }
        if name.starts_with('-') || name.ends_with('-') {
            return Err(ErrorCode::skill_validation(format!(
                "skill name '{name}' must not start or end with a dash"
            )));
        }
        if !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(ErrorCode::skill_validation(format!(
                "skill name '{name}' must contain only lowercase alphanumeric chars or dashes"
            )));
        }
        if name.contains("..") || name.contains('/') || name.contains('\\') {
            return Err(ErrorCode::skill_validation(format!(
                "skill name '{name}' contains path traversal characters"
            )));
        }
        if is_reserved_tool_name(name) {
            return Err(ErrorCode::skill_validation(format!(
                "'{name}' is a reserved tool name"
            )));
        }
        Ok(())
    }

    pub fn validate_file_path(path: &str) -> Result<()> {
        if path.is_empty() {
            return Err(ErrorCode::skill_validation("file path must not be empty"));
        }
        if path.starts_with('/') || path.starts_with('\\') {
            return Err(ErrorCode::skill_validation(format!(
                "file path must be relative, got '{path}'"
            )));
        }
        if path.contains("..") {
            return Err(ErrorCode::skill_validation(format!(
                "file path '{path}' contains '..'"
            )));
        }
        if !ALLOWED_FILE_PREFIXES.iter().any(|p| path.starts_with(p)) {
            return Err(ErrorCode::skill_validation(format!(
                "file path '{path}' must start with one of: {ALLOWED_FILE_PREFIXES:?}"
            )));
        }
        let ext = path.rsplit('.').next().unwrap_or("");
        let allowed = if path.starts_with("scripts/") {
            ALLOWED_SCRIPT_EXTENSIONS
        } else {
            ALLOWED_REFERENCE_EXTENSIONS
        };
        if !allowed.contains(&ext) {
            return Err(ErrorCode::skill_validation(format!(
                "file extension '.{ext}' not allowed for prefix, must be one of: {allowed:?}"
            )));
        }
        Ok(())
    }

    pub fn validate_size(content: &str, files: &[SkillFile]) -> Result<()> {
        if content.len() > MAX_CONTENT_BYTES {
            return Err(ErrorCode::skill_validation(format!(
                "skill content exceeds {MAX_CONTENT_BYTES} byte limit ({} bytes)",
                content.len()
            )));
        }
        let mut total: usize = 0;
        for f in files {
            if f.body.len() > MAX_FILE_BYTES {
                return Err(ErrorCode::skill_validation(format!(
                    "file '{}' exceeds {MAX_FILE_BYTES} byte limit ({} bytes)",
                    f.path,
                    f.body.len()
                )));
            }
            total += f.body.len();
        }
        if total > MAX_TOTAL_FILE_BYTES {
            return Err(ErrorCode::skill_validation(format!(
                "total file size exceeds {MAX_TOTAL_FILE_BYTES} byte limit ({total} bytes)"
            )));
        }
        Ok(())
    }
}

// ── Validation constants ──────────────────────────────────────────────────────

const MAX_CONTENT_BYTES: usize = 10 * 1024;
const MAX_FILE_BYTES: usize = 50 * 1024;
const MAX_TOTAL_FILE_BYTES: usize = 200 * 1024;
const MAX_NAME_LEN: usize = 64;
const MIN_NAME_LEN: usize = 2;

const ALLOWED_FILE_PREFIXES: &[&str] = &["scripts/", "references/"];
const ALLOWED_SCRIPT_EXTENSIONS: &[&str] = &["py", "sh"];
const ALLOWED_REFERENCE_EXTENSIONS: &[&str] = &["md"];

fn is_reserved_tool_name(name: &str) -> bool {
    use crate::kernel::tools::execution::tool_id::ToolId;
    ToolId::ALL.iter().any(|id| id.as_str() == name)
}
