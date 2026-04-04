use std::path::PathBuf;
use std::sync::Arc;

use crate::error::Result;
use crate::store::fs::run::FsRunStore;
use crate::store::fs::session::FsSessionStore;
use crate::store::RunStore;
use crate::store::SessionStore;

pub enum StoreBackend {
    Fs {
        session_dir: PathBuf,
        run_dir: PathBuf,
    },
}

pub struct Stores {
    pub session: Arc<dyn SessionStore>,
    pub run: Arc<dyn RunStore>,
}

pub fn create_stores(backend: StoreBackend) -> Result<Stores> {
    match backend {
        StoreBackend::Fs {
            session_dir,
            run_dir,
        } => Ok(Stores {
            session: Arc::new(FsSessionStore::new(session_dir)),
            run: Arc::new(FsRunStore::new(run_dir)),
        }),
    }
}
