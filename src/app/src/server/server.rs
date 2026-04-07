use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Sse;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use bend_base::logx;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use crate::agent::AppAgent;
use crate::agent::TurnRequest;
use crate::conf::Config;
use crate::error::BendclawError;
use crate::error::Result;
use crate::server::stream;

const INDEX_HTML: &str = include_str!("static/index.html");

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
}

pub struct Server {
    agent: Arc<AppAgent>,
    session_id: RwLock<Option<String>>,
}

impl Server {
    pub fn new(agent: Arc<AppAgent>) -> Arc<Self> {
        Arc::new(Self {
            agent,
            session_id: RwLock::new(None),
        })
    }

    pub async fn start(self: Arc<Self>, host: String, port: u16) -> Result<()> {
        let addr = format!("{host}:{port}");
        logx!(info, "server", "listening", addr = %addr,);

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| BendclawError::Run(format!("failed to bind {addr}: {e}")))?;

        axum::serve(listener, self.router())
            .await
            .map_err(|e| BendclawError::Run(format!("server error: {e}")))?;

        Ok(())
    }

    fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route(
                "/",
                get(|State(server): State<Arc<Server>>| async move { server.index().await }),
            )
            .route(
                "/api/new",
                post(|State(server): State<Arc<Server>>| async move { server.new_session().await }),
            )
            .route(
                "/api/chat",
                post(
                    |State(server): State<Arc<Server>>, Json(req): Json<ChatRequest>| async move {
                        server.chat(req).await
                    },
                ),
            )
            .layer(CorsLayer::permissive())
            .with_state(self)
    }

    async fn index(&self) -> Html<&'static str> {
        Html(INDEX_HTML)
    }

    async fn new_session(&self) -> Json<StatusResponse> {
        *self.session_id.write().await = None;
        Json(StatusResponse {
            status: "ok".into(),
        })
    }

    async fn chat(self: Arc<Self>, req: ChatRequest) -> impl IntoResponse {
        let stream = self.chat_stream(req.message);
        Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("ping"),
        )
    }

    fn chat_stream(
        self: Arc<Self>,
        message: String,
    ) -> impl futures::stream::Stream<
        Item = std::result::Result<axum::response::sse::Event, std::convert::Infallible>,
    > {
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            let session_id = self.session_id.read().await.clone();
            let request = TurnRequest::text(message).session_id(session_id);

            match self.agent.run(request).await {
                Ok(mut turn_stream) => {
                    let sid = turn_stream.session_id.clone();
                    while let Some(event) = turn_stream.next().await {
                        for sse in stream::map_run_event(&event) {
                            if tx.send(sse).await.is_err() {
                                break;
                            }
                        }
                    }
                    *self.session_id.write().await = Some(sid);
                }
                Err(error) => {
                    let _ = tx.send(stream::error_event(error.to_string())).await;
                }
            }

            let _ = tx.send(stream::done_event()).await;
        });

        tokio_stream::wrappers::ReceiverStream::new(rx)
    }
}

pub async fn start(conf: Config) -> Result<()> {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let agent = Arc::new(AppAgent::new(&conf, &cwd)?);

    let storage_backend = match conf.storage.backend {
        crate::conf::StorageBackend::Fs => "fs",
        crate::conf::StorageBackend::Cloud => "cloud",
    };
    let storage_target = match conf.storage.backend {
        crate::conf::StorageBackend::Fs => conf.storage.fs.root_dir.display().to_string(),
        crate::conf::StorageBackend::Cloud => conf.storage.cloud.endpoint.clone(),
    };
    let llm = conf.active_llm();
    let model = llm.model.clone();
    let base_url = llm.base_url.clone().unwrap_or_default();
    let provider = conf.llm.provider.clone();

    let server = Server::new(agent);

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

    server
        .start(conf.server.host.clone(), conf.server.port)
        .await
}
