/// Try to parse JSON, repairing malformed input only when it looks like JSON.
///
/// LLMs sometimes produce truncated or slightly invalid JSON for tool call
/// arguments (trailing commas, missing closing braces, etc.). This function
/// attempts a standard parse first, and only falls back to repair when the
/// input starts with `{`, `[`, or `"` — i.e., it actually looks like JSON.
///
/// Non-JSON strings (plain text, markdown, etc.) are never "repaired" into
/// JSON — the original parse error is returned instead.
pub fn try_repair_json(raw: &str) -> Result<serde_json::Value, serde_json::Error> {
    // Fast path: standard parse
    let err = match serde_json::from_str(raw) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };

    // Only attempt repair on input that looks like JSON
    if looks_like_json(raw) {
        if let Ok(v) = jsonrepair::repair_to_value(raw, &jsonrepair::Options::default()) {
            tracing::warn!("repaired malformed tool-call JSON");
            return Ok(v);
        }
    }

    Err(err)
}

/// Conservative check: does the input start with a JSON structural character?
fn looks_like_json(s: &str) -> bool {
    matches!(s.trim_start().as_bytes().first(), Some(b'{' | b'[' | b'"'))
}
