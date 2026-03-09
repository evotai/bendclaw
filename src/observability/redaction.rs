use serde_json::Map;
use serde_json::Value;

const REDACTED: &str = "[REDACTED]";
const SECRET_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "authorization",
    "token",
    "secret",
    "password",
    "passwd",
    "access_key",
    "private_key",
    "client_secret",
    "refresh_token",
    "session_token",
];

pub fn redact(value: Value) -> Value {
    redact_with_key(None, value)
}

fn redact_with_key(key: Option<&str>, value: Value) -> Value {
    if key.is_some_and(is_secret_key) {
        return redact_leaf(value);
    }

    match value {
        Value::Object(obj) => Value::Object(
            obj.into_iter()
                .map(|(k, v)| {
                    let next = redact_with_key(Some(&k), v);
                    (k, next)
                })
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
        Value::Object(obj) => Value::Object(
            obj.into_iter()
                .map(|(k, v)| (k, redact_leaf(v)))
                .collect::<Map<String, Value>>(),
        ),
        _ => Value::String(REDACTED.to_string()),
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.trim().to_ascii_lowercase();
    SECRET_KEYS.iter().any(|needle| normalized.contains(needle))
}
