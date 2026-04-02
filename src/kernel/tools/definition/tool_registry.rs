use std::collections::HashMap;
use std::sync::Arc;

use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolSpec;
use crate::kernel::tools::tool_id::ToolId;
use crate::llm::tool::ToolSchema;

/// Registry of in-process tools, keyed by name.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn register_builtin(&mut self, id: ToolId, tool: Arc<dyn Tool>) {
        debug_assert_eq!(
            id.as_str(),
            tool.name(),
            "tool id/name mismatch for built-in tool"
        );
        self.register(tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tools.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        let mut specs: Vec<ToolSpec> = self.tools.values().map(|t| t.spec()).collect();
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    pub fn tool_schemas(&self) -> Vec<ToolSchema> {
        let mut schemas: Vec<ToolSchema> = self
            .tools
            .values()
            .map(|t| ToolSchema::new(t.name(), t.description(), t.parameters_schema()))
            .collect();
        schemas.sort_by(|a, b| a.function.name.cmp(&b.function.name));
        schemas
    }

    pub fn get_by_names(&self, names: &[&str]) -> Vec<ToolSchema> {
        names
            .iter()
            .filter_map(|name| {
                self.tools
                    .get(*name)
                    .map(|t| ToolSchema::new(t.name(), t.description(), t.parameters_schema()))
            })
            .collect()
    }

    /// Iterate over all registered tool trait objects.
    pub fn iter_tools(&self) -> impl Iterator<Item = &Arc<dyn Tool>> {
        self.tools.values()
    }

    pub fn get_by_ids(&self, ids: &[ToolId]) -> Vec<ToolSchema> {
        ids.iter()
            .filter_map(|id| {
                self.tools
                    .get(id.as_str())
                    .map(|t| ToolSchema::new(t.name(), t.description(), t.parameters_schema()))
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
