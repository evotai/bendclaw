use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use super::tool_definition::ToolDefinition;
use super::tool_target::ToolTarget;
use crate::llm::tool::ToolSchema;

/// A registered tool: pure metadata paired with its dispatch target.
pub struct ToolEntry {
    pub definition: ToolDefinition,
    pub target: ToolTarget,
}

#[derive(Clone)]
pub struct Toolset {
    /// Pure metadata for all tools (builtin + skill). Prompt-safe.
    pub definitions: Arc<Vec<ToolDefinition>>,
    /// Runtime dispatch targets, keyed by tool name.
    pub bindings: Arc<HashMap<String, ToolTarget>>,
    /// LLM-facing schemas, derived from definitions. Cached for reuse.
    pub tools: Arc<Vec<ToolSchema>>,
    /// Optional filter restricting which tools are active.
    pub allowed_tool_names: Option<HashSet<String>>,
}

impl Toolset {
    /// Build from a list of tool entries with optional filter.
    pub fn from_entries(entries: Vec<ToolEntry>, filter: Option<HashSet<String>>) -> Self {
        let (defs, bindings, allowed) = match filter {
            Some(names) => {
                let filtered: Vec<ToolEntry> = entries
                    .into_iter()
                    .filter(|e| names.contains(&e.definition.name))
                    .collect();
                let allowed: HashSet<String> =
                    filtered.iter().map(|e| e.definition.name.clone()).collect();
                let (defs, binds) = split_entries(filtered);
                (defs, binds, Some(allowed))
            }
            None => {
                let (defs, binds) = split_entries(entries);
                (defs, binds, None)
            }
        };

        let tools = Arc::new(defs.iter().map(|d| d.to_tool_schema()).collect());
        Self {
            definitions: Arc::new(defs),
            bindings: Arc::new(bindings),
            tools,
            allowed_tool_names: allowed,
        }
    }

    /// Append skill entries to this toolset.
    pub fn append_skill_entries(&mut self, entries: Vec<ToolEntry>) {
        let existing: HashSet<&str> = self.definitions.iter().map(|d| d.name.as_str()).collect();
        let mut defs = self.definitions.as_ref().clone();
        let mut binds = self.bindings.as_ref().clone();
        let mut schemas = self.tools.as_ref().clone();

        for entry in entries {
            if existing.contains(entry.definition.name.as_str()) {
                continue;
            }
            if let Some(ref f) = self.allowed_tool_names {
                if !f.contains(&entry.definition.name) {
                    continue;
                }
            }
            if let Some(ref mut allowed) = self.allowed_tool_names {
                allowed.insert(entry.definition.name.clone());
            }
            schemas.push(entry.definition.to_tool_schema());
            binds.insert(entry.definition.name.clone(), entry.target);
            defs.push(entry.definition);
        }

        self.definitions = Arc::new(defs);
        self.bindings = Arc::new(binds);
        self.tools = Arc::new(schemas);
    }

    /// Look up a definition by name.
    pub fn get_definition(&self, name: &str) -> Option<&ToolDefinition> {
        self.definitions.iter().find(|d| d.name == name)
    }

    /// Look up a dispatch target by name.
    pub fn get_target(&self, name: &str) -> Option<&ToolTarget> {
        self.bindings.get(name)
    }
}

fn split_entries(entries: Vec<ToolEntry>) -> (Vec<ToolDefinition>, HashMap<String, ToolTarget>) {
    let mut defs = Vec::with_capacity(entries.len());
    let mut binds = HashMap::with_capacity(entries.len());
    for entry in entries {
        binds.insert(entry.definition.name.clone(), entry.target);
        defs.push(entry.definition);
    }
    (defs, binds)
}
