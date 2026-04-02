use std::path::Path;

use crate::types::ErrorCode;
use crate::types::Result;

/// Resolve a string that may be a `@file` reference.
/// If the string starts with `@`, read the file contents.
/// Otherwise return the string as-is.
pub fn resolve_at_file(s: &str) -> Result<String> {
    if let Some(path) = s.strip_prefix('@') {
        let path = Path::new(path);
        std::fs::read_to_string(path).map_err(|e| {
            ErrorCode::invalid_input(format!("failed to read @file '{}': {e}", path.display()))
        })
    } else {
        Ok(s.to_string())
    }
}
