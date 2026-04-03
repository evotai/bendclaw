//! Skill argument parsing for tool call dispatch.

use crate::kernel::skills::diagnostics;

/// Parse JSON tool call arguments into CLI args for the skill executor.
pub fn parse_skill_args(skill_name: &str, arguments: &str) -> Vec<String> {
    let parsed: serde_json::Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(e) => {
            diagnostics::log_skill_args_parse_failed(skill_name, &e);
            return vec![];
        }
    };

    let mut args = Vec::new();
    if let Some(obj) = parsed.as_object() {
        for (key, value) in obj {
            args.push(format!("--{key}"));
            match value {
                serde_json::Value::String(s) => args.push(s.clone()),
                other => args.push(other.to_string()),
            }
        }
    }
    args
}
