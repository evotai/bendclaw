use std::sync::Arc;

use crate::kernel::run::persist_op::PersistWriter;
use crate::kernel::run::prompt::prompt_model::PromptConfig;
use crate::kernel::session::backend::persistent::PersistentBackend;
use crate::kernel::session::store::json::JsonSessionStore;
use crate::storage::Pool;

pub fn build_local_backend(
    store: Arc<JsonSessionStore>,
    persist_writer: PersistWriter,
    session_id: &str,
) -> Arc<PersistentBackend<JsonSessionStore>> {
    Arc::new(PersistentBackend::new(
        store,
        persist_writer,
        session_id,
        "local",
        "cli",
        None,
    ))
}

pub fn build_cloud_backend(
    pool: Pool,
    persist_writer: PersistWriter,
    session_id: &str,
    agent_id: &str,
    user_id: &str,
    prompt_config: Option<PromptConfig>,
) -> (
    Arc<crate::kernel::session::store::db::DbSessionStore>,
    Arc<PersistentBackend<crate::kernel::session::store::db::DbSessionStore>>,
) {
    let session_store = Arc::new(crate::kernel::session::store::db::DbSessionStore::new(pool));
    let persistent = Arc::new(PersistentBackend::new(
        session_store.clone(),
        persist_writer,
        session_id,
        agent_id,
        user_id,
        prompt_config,
    ));
    (session_store, persistent)
}
