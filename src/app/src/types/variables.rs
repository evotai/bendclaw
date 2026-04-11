//! Variable domain model — flat key/value records and persistence document.

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableRecord {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariablesDocument {
    pub version: u32,
    pub variables: Vec<VariableRecord>,
}
