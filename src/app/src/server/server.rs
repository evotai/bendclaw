use std::sync::Arc;

use axum::routing::get;
use axum::routing::post;
use axum::Router;
use bend_base::logx;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;

use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::server::handler;
use crate::storage::open_storage;
use crate::storage::Storage;

pub(crate) struct AppState {
    pub(crate) llm: LlmConfig,
    pub(crate) storage: Arc<dyn Storage>,
    pub(crate) session_id: Mutex<Option<String>>,
}

pub async fn start(conf: Config) -> Result<()> {
    let llm = conf.active_llm();
    let storage = open_storage(&conf.storage)?;
    let storage_backend = match conf.storage.backend {
        crate::conf::StorageBackend::Fs => "fs",
        crate::conf::StorageBackend::Cloud => "cloud",
    };
    let storage_target = match conf.storage.backend {
        crate::conf::StorageBackend::Fs => conf.storage.fs.root_dir.display().to_string(),
        crate::conf::StorageBackend::Cloud => conf.storage.cloud.endpoint.clone(),
    };
    let model = llm.model.clone();
    let base_url = llm.base_url.clone().unwrap_or_default();
    let provider = conf.llm.provider.clone();

    let state = Arc::new(AppState {
        llm,
        storage,
        session_id: Mutex::new(None),
    });

    let app = Router::new()
        .route("/", get(handler::index))
        .route("/api/new", post(handler::new_session))
        .route("/api/chat", post(handler::chat))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", conf.server.host, conf.server.port);
    logx!(
        info,
        "server",
        "configured",
        addr = %addr,
        provider = ?provider,
        model = %model,
        base_url = %base_url,
        storage_backend = storage_backend,
        storage_target = %storage_target,
    );
    logx!(info, "server", "listening", addr = %addr,);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| BendclawError::Run(format!("failed to bind {addr}: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| BendclawError::Run(format!("server error: {e}")))?;

    Ok(())
}
