use std::path::Path;

use fs2::FileExt;

use crate::error::EvotError;
use crate::error::Result;
use crate::storage::FavoritesEdit;
use crate::storage::FavoritesUpdate;
use crate::types::FavoritesDocument;

pub(super) fn edit(path: &Path, edit: FavoritesEdit) -> Result<FavoritesUpdate> {
    let parent = path
        .parent()
        .ok_or_else(|| EvotError::Store("favorites path has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join("favorites.lock"))?;
    FileExt::lock_exclusive(&lock)?;
    let result = (|| {
        let mut doc = match std::fs::read_to_string(path) {
            Ok(json) => serde_json::from_str::<FavoritesDocument>(&json)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => FavoritesDocument {
                version: 1,
                ids: Vec::new(),
            },
            Err(error) => return Err(EvotError::Io(error)),
        };
        let before = doc.ids.len();
        if edit.apply(&mut doc.ids) {
            // Keep the existing on-disk format and version; only ownership of
            // the read/modify/write transaction changes here.
            crate::atomic_file::write_private_atomic(
                path,
                serde_json::to_string_pretty(&doc)?.as_bytes(),
            )?;
        }
        Ok(FavoritesUpdate {
            removed: before.saturating_sub(doc.ids.len()),
            ids: doc.ids,
        })
    })();
    FileExt::unlock(&lock)?;
    result
}
