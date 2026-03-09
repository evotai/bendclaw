pub fn new_id() -> String {
    ulid::Ulid::new().to_string().to_lowercase()
}

pub fn new_prefixed_id(prefix: &str) -> String {
    format!("{}{}", prefix, ulid::Ulid::new().to_string().to_lowercase())
}

pub fn new_os_id() -> String {
    new_prefixed_id("os")
}

pub fn new_agent_id() -> String {
    new_prefixed_id("a")
}

pub fn new_session_id() -> String {
    new_prefixed_id("s")
}

pub fn new_run_id() -> String {
    new_prefixed_id("r")
}

pub fn sanitize_agent_id(id: &str) -> String {
    let mut result = String::with_capacity(id.len());
    for ch in id.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
        } else if !result.ends_with('_') {
            result.push('_');
        }
    }
    let trimmed = result.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "default".to_string()
    } else {
        trimmed
    }
}
