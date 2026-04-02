use crate::types::ErrorCode;
use crate::types::Result;

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

pub fn validate_agent_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(ErrorCode::invalid_input("agent_id must not be empty"));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(ErrorCode::invalid_input(format!(
            "invalid agent_id '{id}' (only [a-zA-Z0-9_-] allowed)"
        )));
    }
    Ok(())
}
