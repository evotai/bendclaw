use std::path::PathBuf;

use crate::auth::types::AuthState;
use crate::auth::types::ModelsCache;
use crate::conf::paths;
use crate::error::EvotError;
use crate::error::Result;

pub fn auth_file_path() -> Result<PathBuf> {
    Ok(paths::state_root_dir()?.join("auth.json"))
}

pub fn models_cache_path() -> Result<PathBuf> {
    Ok(paths::state_root_dir()?.join("models.cache.json"))
}

pub fn load_auth() -> Result<Option<AuthState>> {
    let path = auth_file_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| EvotError::Conf(format!("read {}: {error}", path.display())))?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let state = serde_json::from_str::<AuthState>(&raw)
        .map_err(|error| EvotError::Conf(format!("parse {}: {error}", path.display())))?;
    Ok(Some(state))
}

pub fn save_auth(state: &AuthState) -> Result<()> {
    let path = auth_file_path()?;
    write_private(&path, &serde_json::to_string_pretty(state)?)
}

pub fn clear_auth() -> Result<()> {
    let path = auth_file_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn load_models_cache() -> Result<Option<ModelsCache>> {
    let path = models_cache_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| EvotError::Conf(format!("read {}: {error}", path.display())))?;
    let cache = serde_json::from_str::<ModelsCache>(&raw)
        .map_err(|error| EvotError::Conf(format!("parse {}: {error}", path.display())))?;
    Ok(Some(cache))
}

pub fn save_models_cache(cache: &ModelsCache) -> Result<()> {
    let path = models_cache_path()?;
    write_private(&path, &serde_json::to_string_pretty(cache)?)
}

fn write_private(path: &PathBuf, content: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}
