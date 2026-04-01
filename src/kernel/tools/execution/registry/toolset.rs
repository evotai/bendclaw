use std::collections::HashSet;
use std::sync::Arc;

use super::tool_registry::ToolRegistry;
use crate::kernel::tools::ToolId;
use crate::llm::tool::ToolSchema;

#[derive(Clone)]
pub struct Toolset {
    pub registry: Arc<ToolRegistry>,
    pub tools: Arc<Vec<ToolSchema>>,
    pub allowed_tool_names: Option<HashSet<String>>,
}

impl Toolset {
    pub fn from_registry(
        registry: ToolRegistry,
        filter: Option<HashSet<String>>,
        default_ids: &[ToolId],
    ) -> Self {
        let registry = Arc::new(registry);
        let (tools, allowed_tool_names) = match filter {
            Some(names) => {
                let schemas: Vec<ToolSchema> = registry
                    .tool_schemas()
                    .into_iter()
                    .filter(|t| names.contains(&t.function.name))
                    .collect();
                let allowed: HashSet<String> =
                    schemas.iter().map(|t| t.function.name.clone()).collect();
                (schemas, Some(allowed))
            }
            None => {
                let schemas = registry.get_by_ids(default_ids);
                (schemas, None)
            }
        };
        Self {
            registry,
            tools: Arc::new(tools),
            allowed_tool_names,
        }
    }
}
