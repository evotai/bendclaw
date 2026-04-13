use serde_json::Map;
use serde_json::Value;

const REDACTED: &str = "[REDACTED]";
const SECRET_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "authorization",
    "secret",
    "password",
    "passwd",
    "access_key",
    "private_key",
    "client_secret",
    "refresh_token",
    "session_token",
];

const SAFE_KEYS: &[&str] = &[
    "prompt_tokens",
    "completion_tokens",
    "total_tokens",
    "cached_tokens",
    "reasoning_tokens",
    "input_tokens",
    "output_tokens",
    "token_count",
    "max_tokens",
];

pub fn redact_value(value: Value) -> Value {
    redact_with_key(None, value)
}

fn redact_with_key(key: Option<&str>, value: Value) -> Value {
    if key.is_some_and(is_secret_key) {
        return redact_leaf(value);
    }

    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key.clone(), redact_with_key(Some(&key), value)))
                .collect::<Map<String, Value>>(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| redact_with_key(None, item))
                .collect(),
        ),
        other => other,
    }
}

fn redact_leaf(value: Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Array(items) => Value::Array(items.into_iter().map(redact_leaf).collect()),
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, redact_leaf(value)))
                .collect::<Map<String, Value>>(),
        ),
        _ => Value::String(REDACTED.to_string()),
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.trim().to_ascii_lowercase();
    if SAFE_KEYS.iter().any(|safe| normalized == *safe) {
        return false;
    }

    SECRET_KEYS.iter().any(|needle| normalized.contains(needle))
}
