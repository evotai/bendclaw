//! Session preparation, independent of providers, runs and access adapters.
//!
//! The caller supplies a frozen model selection. Existing sessions retain their
//! original workspace and source; an explicit workspace applies only at creation.
use std::sync::Arc;

use super::Session;
use crate::error::EvotError;
use crate::error::Result;
use crate::storage::Storage;
use crate::types::SessionMeta;

pub struct SessionSelection {
    pub provider: String,
    pub model: String,
    pub thinking_level: Option<String>,
}

pub struct SessionService {
    storage: Arc<dyn Storage>,
    default_workspace: String,
}

impl SessionService {
    pub fn new(storage: Arc<dyn Storage>, default_workspace: String) -> Self {
        Self {
            storage,
            default_workspace,
        }
    }

    pub async fn load(&self, id: &str) -> Result<Option<Arc<Session>>> {
        Session::open(id, self.storage.clone()).await
    }

    /// Blank drafts do not stamp reasoning effort until a run begins.
    pub async fn create(
        &self,
        source: &str,
        cwd: Option<&str>,
        provider: String,
        model: String,
    ) -> Result<SessionMeta> {
        let session = Session::new_with_provider_source(
            crate::types::new_id(),
            self.creation_workspace(cwd)?,
            provider,
            model,
            source,
            self.storage.clone(),
        )
        .await?;
        Ok(session.meta().await)
    }

    /// Resolve a run's session, preserving existing workspace/source metadata.
    /// Admission and clear/delete synchronization stay with the run owner.
    pub async fn resolve(
        &self,
        id: Option<&str>,
        source: &str,
        selection: SessionSelection,
        cwd: Option<&str>,
    ) -> Result<Arc<Session>> {
        let SessionSelection {
            provider,
            model,
            thinking_level,
        } = selection;
        let existing = match id {
            Some(id) => self.load(id).await?,
            None => None,
        };
        let session = match existing {
            Some(session) => {
                session.set_model_selection(provider, model).await?;
                session
            }
            None => {
                Session::new_with_provider_source(
                    id.map(str::to_owned).unwrap_or_else(crate::types::new_id),
                    self.creation_workspace(cwd)?,
                    provider,
                    model,
                    source,
                    self.storage.clone(),
                )
                .await?
            }
        };
        session.set_thinking_level(thinking_level).await;
        Ok(session)
    }

    fn creation_workspace(&self, requested: Option<&str>) -> Result<String> {
        match requested {
            Some(path) => canonical_workspace(path),
            None => Ok(self.default_workspace.clone()),
        }
    }
}

fn canonical_workspace(cwd: &str) -> Result<String> {
    let path = crate::conf::paths::expand_home_path(cwd.trim())?;
    if path.as_os_str().is_empty() {
        return Err(EvotError::Conf("workspace path must not be empty".into()));
    }
    let metadata = std::fs::metadata(&path).map_err(|error| {
        EvotError::Conf(format!(
            "workspace '{}' is not accessible: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(EvotError::Conf(format!(
            "workspace '{}' is not a directory",
            path.display()
        )));
    }
    let canonical = std::fs::canonicalize(&path).unwrap_or(path);
    Ok(canonical.to_string_lossy().into_owned())
}
