use std::fmt;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorSource {
    Llm,
    Tool(String),
    Skill(String),
    Sandbox,
    Internal,
}

impl fmt::Display for ErrorSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Llm => f.write_str("llm"),
            Self::Tool(name) => write!(f, "tool:{name}"),
            Self::Skill(name) => write!(f, "skill:{name}"),
            Self::Sandbox => f.write_str("sandbox"),
            Self::Internal => f.write_str("internal"),
        }
    }
}

impl Serialize for ErrorSource {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ErrorSource {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(if s == "llm" {
            Self::Llm
        } else if s == "sandbox" {
            Self::Sandbox
        } else if s == "internal" {
            Self::Internal
        } else if let Some(name) = s.strip_prefix("tool:") {
            Self::Tool(name.to_string())
        } else if let Some(name) = s.strip_prefix("skill:") {
            Self::Skill(name.to_string())
        } else {
            Self::Internal
        })
    }
}
