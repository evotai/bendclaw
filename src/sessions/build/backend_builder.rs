use std::sync::Arc;

use crate::execution::persist::persist_op::PersistWriter;
use crate::planning::prompt_model::PromptConfig;
use crate::sessions::backend::persistent::PersistentBackend;
use crate::sessions::store::json::JsonSessionStore;
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
    Arc<crate::sessions::store::db::DbSessionStore>,
    Arc<PersistentBackend<crate::sessions::store::db::DbSessionStore>>,
) {
    let session_store = Arc::new(crate::sessions::store::db::DbSessionStore::new(pool));
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
