//! Per-user remote skill directory paths.

use std::path::Path;
use std::path::PathBuf;

/// `{workspace_root}/users/{user_id}/skills/.remote/`
pub fn remote_dir(workspace_root: &Path, user_id: &str) -> PathBuf {
    workspace_root
        .join("users")
        .join(user_id)
        .join("skills")
        .join(".remote")
}

/// `{workspace_root}/users/{user_id}/skills/.remote/{skill_name}/`
pub fn skill_dir(workspace_root: &Path, user_id: &str, skill_name: &str) -> PathBuf {
    remote_dir(workspace_root, user_id).join(skill_name)
}

/// `{workspace_root}/users/{subscriber_id}/skills/.remote/subscribed/{owner_id}/`
pub fn subscribed_dir(workspace_root: &Path, subscriber_id: &str, owner_id: &str) -> PathBuf {
    remote_dir(workspace_root, subscriber_id)
        .join("subscribed")
        .join(owner_id)
}

/// `{workspace_root}/users/{subscriber_id}/skills/.remote/subscribed/{owner_id}/{skill_name}/`
pub fn subscribed_skill_dir(
    workspace_root: &Path,
    subscriber_id: &str,
    owner_id: &str,
    skill_name: &str,
) -> PathBuf {
    subscribed_dir(workspace_root, subscriber_id, owner_id).join(skill_name)
}
