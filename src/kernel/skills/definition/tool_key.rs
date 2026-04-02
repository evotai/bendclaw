//! Tool key protocol — single source of truth for skill tool key format.
//!
//! Tool key: bare "name" for owned/hub skills, "owner_id/name" for subscribed.

use super::skill::Skill;

/// Format a skill's tool key for a given viewer.
/// Owned/hub: bare name. Subscribed: "owner_id/name".
pub fn format(skill: &Skill, viewer_id: &str) -> String {
    if skill.user_id == viewer_id || skill.user_id.is_empty() {
        skill.name.clone()
    } else {
        format_subscribed(&skill.user_id, &skill.name)
    }
}

/// Format a subscribed skill's tool key: "owner_id/name".
pub fn format_subscribed(owner_id: &str, name: &str) -> String {
    format!("{}/{}", owner_id, name)
}

/// Parse a tool key into (owner_id, bare_name).
/// If no "/" present, owner defaults to `default_owner`.
pub fn parse<'a>(tool_key: &'a str, default_owner: &'a str) -> (&'a str, &'a str) {
    match tool_key.find('/') {
        Some(idx) => (&tool_key[..idx], &tool_key[idx + 1..]),
        None => (default_owner, tool_key),
    }
}
