//! Memory tool — persistent two-layer memory for the agent.
//!
//! Provides `add`, `replace`, `remove`, and `read` operations across
//! global (cross-project) and project (codebase-specific) scopes.
//! The tool handles frontmatter generation, security scanning, quota
//! enforcement, and automatic MEMORY.md index rebuilding.

use std::path::PathBuf;

use async_trait::async_trait;

use super::scan::scan_content;
use super::store::MemoryKind;
use super::store::MemoryScope;
use super::store::MemoryStore;
use crate::types::*;

const TOOL_DESCRIPTION: &str = "\
Manage persistent memory across sessions. Memory is injected into future \
system prompts, so keep entries compact and factual.

WHEN TO SAVE (proactively, don't wait to be asked):
- User corrects you or says 'remember this'
- User shares a preference or personal detail
- You notice a recurring pattern in the user's behavior
- You discover an environment fact or project convention

SCOPE:
- global: cross-project (user identity, preferences, general feedback)
- project: this codebase only (architecture decisions, project-specific feedback)

Rule: would this matter in a different project? -> global. Only this repo? -> project.";

pub struct MemoryTool {
    global_dir: PathBuf,
    project_dir: PathBuf,
    disallow_writes: Option<String>,
}

impl MemoryTool {
    pub fn new(global_dir: PathBuf, project_dir: PathBuf) -> Self {
        Self {
            global_dir,
            project_dir,
            disallow_writes: None,
        }
    }

    pub fn disallow_writes(mut self, msg: impl Into<String>) -> Self {
        self.disallow_writes = Some(msg.into());
        self
    }

    fn store(&self) -> MemoryStore {
        MemoryStore::new(self.global_dir.clone(), self.project_dir.clone())
    }
}

#[async_trait]
impl AgentTool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn label(&self) -> &str {
        "Memory"
    }

    fn description(&self) -> &str {
        TOOL_DESCRIPTION
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "replace", "remove", "read"],
                    "description": "add: create new (fails if name exists). replace: overwrite existing. remove: delete. read: list all or read one."
                },
                "scope": {
                    "type": "string",
                    "enum": ["global", "project"],
                    "description": "global: cross-project. project: this codebase only."
                },
                "name": {
                    "type": "string",
                    "description": "Stable identifier (used as filename). Required for add/replace/remove. Optional for read (omit to list all)."
                },
                "type": {
                    "type": "string",
                    "enum": ["user", "feedback", "project", "reference"],
                    "description": "Memory category. Required for add/replace."
                },
                "description": {
                    "type": "string",
                    "description": "One-line summary for the index. Required for add/replace."
                },
                "content": {
                    "type": "string",
                    "description": "Memory body text. Required for add/replace."
                }
            },
            "required": ["action", "scope"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let action = params["action"].as_str().unwrap_or("?");
        let scope = params["scope"].as_str().unwrap_or("?");
        let name = params["name"].as_str().unwrap_or("");
        if name.is_empty() {
            Some(format!("memory {action} {scope}"))
        } else {
            Some(format!("memory {action} {scope}/{name}"))
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let action = params["action"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'action' parameter".into()))?;
        let scope_str = params["scope"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'scope' parameter".into()))?;
        let scope = MemoryScope::parse(scope_str).map_err(ToolError::InvalidArgs)?;

        // Block writes in planning mode
        if matches!(action, "add" | "replace" | "remove") {
            if let Some(msg) = &self.disallow_writes {
                return Err(ToolError::Failed(format!("Error: {msg}")));
            }
        }

        let name = params["name"].as_str();
        let store = self.store();

        let result = match action {
            "read" => store.read(scope, name),

            "add" | "replace" => {
                let name =
                    name.ok_or_else(|| ToolError::InvalidArgs("missing 'name' parameter".into()))?;
                let kind_str = params["type"]
                    .as_str()
                    .ok_or_else(|| ToolError::InvalidArgs("missing 'type' parameter".into()))?;
                let kind = MemoryKind::parse(kind_str).map_err(ToolError::InvalidArgs)?;
                let description = params["description"].as_str().ok_or_else(|| {
                    ToolError::InvalidArgs("missing 'description' parameter".into())
                })?;
                let content = params["content"]
                    .as_str()
                    .ok_or_else(|| ToolError::InvalidArgs("missing 'content' parameter".into()))?;

                // Security scan on the body text
                if let Some(reason) = scan_content(content) {
                    return Err(ToolError::Failed(reason));
                }
                // Also scan description
                if let Some(reason) = scan_content(description) {
                    return Err(ToolError::Failed(reason));
                }

                if action == "add" {
                    store.add(scope, name, description, kind, content)
                } else {
                    store.replace(scope, name, description, kind, content)
                }
            }

            "remove" => {
                let name =
                    name.ok_or_else(|| ToolError::InvalidArgs("missing 'name' parameter".into()))?;
                store.remove(scope, name)
            }

            _ => Err(format!(
                "Unknown action '{action}'. Use: add, replace, remove, read."
            )),
        };

        match result {
            Ok(text) => Ok(ToolResult {
                content: vec![Content::Text { text }],
                details: serde_json::json!({}),
                retention: Retention::Normal,
            }),
            Err(msg) => Err(ToolError::Failed(msg)),
        }
    }
}
