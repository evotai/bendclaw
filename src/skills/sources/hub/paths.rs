//! Hub skill directory paths.

use std::path::Path;
use std::path::PathBuf;

pub const SYNC_MARKER: &str = ".evot-hub-sync";

/// `{workspace_root}/skills/.hub/`
pub fn hub_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("skills").join(".hub")
}
