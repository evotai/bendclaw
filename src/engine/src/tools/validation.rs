/// Lightweight tool-input validation and type coercion.
///
/// Inspired by Claude Code's `toolErrors.ts` (structured error messages) and
/// Forge Code's `schema_coercion.rs` (best-effort type repair).  The goal is
/// to catch the most common LLM mistakes — missing required params, wrong
/// primitive types — *before* `tool.execute()` runs, and to silently fix
/// trivial type mismatches (string→integer, string→boolean) so the model
/// doesn't waste a round-trip.
///
/// Only the JSON Schema subset actually used by bendclaw tools is supported:
/// flat objects, `required`, `properties.*.type`, `properties.*.enum`.
///
/// Not supported (and silently ignored): nested/recursive schemas, `anyOf`,
/// `oneOf`, `$ref`, `additionalProperties`.  Unknown `type` values are
/// treated as valid to avoid false rejections.
use serde_json::Value;

// ── public API ──────────────────────────────────────────────────────────

/// Validate `input` against `schema` and coerce trivial type mismatches.
///
/// Returns `Ok(coerced_input)` on success, or `Err(structured_error)` with a
/// human-/LLM-readable message listing every problem found.
pub fn validate_and_coerce(
    tool_name: &str,
    schema: &Value,
    input: &Value,
) -> Result<Value, String> {
    // If schema has no "properties" key we cannot validate — pass through.
    let props = match schema.get("properties").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return Ok(input.clone()),
    };

    let obj = match input.as_object() {
        Some(o) => o,
        None => {
            return Err(format_error(tool_name, &[
                "Tool input must be a JSON object".to_string(),
            ]));
        }
    };

    let mut errors: Vec<String> = Vec::new();
    let mut coerced = obj.clone();

    // ── required ────────────────────────────────────────────────────────
    if let Some(req) = schema.get("required").and_then(|v| v.as_array()) {
        for r in req {
            if let Some(name) = r.as_str() {
                if !coerced.contains_key(name) {
                    errors.push(format!("The required parameter `{name}` is missing"));
                }
            }
        }
    }

    // ── per-property type check + coerce ────────────────────────────────
    for (key, prop_schema) in props {
        let val = match coerced.get(key) {
            Some(v) => v.clone(),
            None => continue, // missing optionals are fine
        };

        // type coerce first — so enum check below sees the coerced value
        let val = if let Some(expected_type) = prop_schema.get("type").and_then(|v| v.as_str()) {
            match try_coerce(&val, expected_type) {
                CoerceResult::Ok(v) => {
                    coerced.insert(key.clone(), v.clone());
                    v
                }
                CoerceResult::AlreadyCorrect => val,
                CoerceResult::Mismatch => {
                    let actual = json_type_name(&val);
                    errors.push(format!(
                        "The parameter `{key}` type is expected as `{expected_type}` but provided as `{actual}`"
                    ));
                    continue;
                }
            }
        } else {
            val
        };

        // enum check (on the coerced value)
        if let Some(allowed) = prop_schema.get("enum").and_then(|v| v.as_array()) {
            if !allowed.contains(&val) {
                let options: Vec<String> = allowed.iter().map(|v| format!("{v}")).collect();
                errors.push(format!(
                    "The parameter `{key}` value {val} is not one of the allowed values: [{}]",
                    options.join(", ")
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(Value::Object(coerced))
    } else {
        Err(format_error(tool_name, &errors))
    }
}

/// Truncate an error string that exceeds 10 000 characters, keeping the first
/// and last 5 000 characters with a note in the middle.  Uses `char_indices`
/// so the cut points always land on valid UTF-8 boundaries.
pub fn truncate_error(text: &str) -> String {
    const MAX: usize = 10_000;
    const HALF: usize = 5_000;
    if text.len() <= MAX {
        return text.to_string();
    }
    // Find the char boundary at or before HALF bytes from the start.
    let start_end = text
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= HALF)
        .last()
        .unwrap_or(0);
    // Find the char boundary at or after (len - HALF) bytes from the end.
    let tail_start = text
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| i >= text.len() - HALF)
        .unwrap_or(text.len());
    let truncated = tail_start - start_end;
    format!(
        "{}\n\n... [{truncated} characters truncated] ...\n\n{}",
        &text[..start_end],
        &text[tail_start..]
    )
}

// ── internals ───────────────────────────────────────────────────────────

enum CoerceResult {
    /// Value was coerced to a new value.
    Ok(Value),
    /// Value already matches the expected type.
    AlreadyCorrect,
    /// Value cannot be coerced.
    Mismatch,
}

fn try_coerce(val: &Value, expected: &str) -> CoerceResult {
    // Already the right type?
    if type_matches(val, expected) {
        return CoerceResult::AlreadyCorrect;
    }

    // Only attempt coercion from strings.
    let s = match val.as_str() {
        Some(s) => s,
        None => return CoerceResult::Mismatch,
    };

    match expected {
        "integer" => {
            if let Ok(n) = s.parse::<i64>() {
                return CoerceResult::Ok(Value::Number(n.into()));
            }
            if let Ok(n) = s.parse::<u64>() {
                return CoerceResult::Ok(Value::Number(n.into()));
            }
            CoerceResult::Mismatch
        }
        "number" => {
            if let Ok(n) = s.parse::<i64>() {
                return CoerceResult::Ok(Value::Number(n.into()));
            }
            if let Ok(n) = s.parse::<f64>() {
                if let Some(num) = serde_json::Number::from_f64(n) {
                    return CoerceResult::Ok(Value::Number(num));
                }
            }
            CoerceResult::Mismatch
        }
        "boolean" => match s.trim().to_lowercase().as_str() {
            "true" => CoerceResult::Ok(Value::Bool(true)),
            "false" => CoerceResult::Ok(Value::Bool(false)),
            _ => CoerceResult::Mismatch,
        },
        "array" => {
            if let Ok(v) = serde_json::from_str::<Value>(s) {
                if v.is_array() {
                    return CoerceResult::Ok(v);
                }
            }
            CoerceResult::Mismatch
        }
        "object" => {
            if let Ok(v) = serde_json::from_str::<Value>(s) {
                if v.is_object() {
                    return CoerceResult::Ok(v);
                }
            }
            CoerceResult::Mismatch
        }
        _ => CoerceResult::Mismatch,
    }
}

fn type_matches(val: &Value, expected: &str) -> bool {
    match expected {
        "string" => val.is_string(),
        "integer" => val.is_i64() || val.is_u64(),
        "number" => val.is_number(),
        "boolean" => val.is_boolean(),
        "array" => val.is_array(),
        "object" => val.is_object(),
        "null" => val.is_null(),
        _ => true, // unknown type — don't block
    }
}

fn json_type_name(val: &Value) -> &'static str {
    match val {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn format_error(tool_name: &str, issues: &[String]) -> String {
    let label = if issues.len() == 1 { "issue" } else { "issues" };
    let body = issues.join("\n");
    format!("InputValidationError: {tool_name} failed due to the following {label}:\n{body}")
}
