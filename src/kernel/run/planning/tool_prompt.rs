//! Tool prompt rendering — generates the "Available Tools" section from ToolDefinitions.

use std::fmt::Write;

use super::prompt_model::truncate_layer;
use super::prompt_model::MAX_TOOLS_BYTES;
use crate::kernel::tools::definition::tool_definition::ToolDefinition;

/// Render the tools section of the system prompt from unified tool definitions.
pub fn render_tools_section(prompt: &mut String, definitions: &[ToolDefinition]) {
    if definitions.is_empty() {
        return;
    }
    let mut buf = String::new();
    buf.push_str("## Available Tools\n\n");
    for d in definitions {
        let _ = writeln!(buf, "- `{}`: {}", d.name, d.description);
    }
    buf.push_str(
        "\nCall tools when they would help accomplish the task.\
         \nTo self-upgrade, run `bendclaw update && bendclaw restart` via shell. Warn the user that the session will be interrupted.\n",
    );
    prompt.push_str(&truncate_layer("tools", &buf, MAX_TOOLS_BYTES, "registry"));
}
