use std::collections::HashSet;
use std::sync::Arc;

use crate::kernel::tools::execution::registry::toolset::Toolset;
use crate::llm::tool::ToolSchema;

pub fn append_skill_schemas(
    toolset: &mut Toolset,
    skills: &[(String, String, serde_json::Value)],
    filter: &Option<HashSet<String>>,
) {
    let existing_names: HashSet<String> = toolset
        .tools
        .iter()
        .map(|t| t.function.name.clone())
        .collect();
    let mut tools = toolset.tools.as_ref().clone();
    for (name, desc, params) in skills {
        if existing_names.contains(name) {
            continue;
        }
        if let Some(ref f) = filter {
            if !f.contains(name) {
                continue;
            }
        }
        tools.push(ToolSchema::new(name, desc, params.clone()));
        if let Some(ref mut allowed) = toolset.allowed_tool_names {
            allowed.insert(name.clone());
        }
    }
    toolset.tools = Arc::new(tools);
}
