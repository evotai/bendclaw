use serde::Deserialize;
use serde::Serialize;

/// JSON Schema description of a tool/function the LLM can call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub function: FunctionDef,
}

/// Function definition within a tool schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolSchema {
    pub fn new(name: &str, description: &str, parameters: serde_json::Value) -> Self {
        Self {
            schema_type: "function".to_string(),
            function: FunctionDef {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
            },
        }
    }
}
