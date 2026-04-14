use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Sse;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use crate::agent::Agent;
use crate::agent::QueryRequest;
use crate::error::EvotError;
use crate::error::Result;
use crate::gateway::channels::http::stream;

const INDEX_HTML: &str = include_str!("static/index.html");

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session_id: Option<String>,
}

pub struct Server {
    agent: Arc<Agent>,
}

impl Server {
    pub fn new(agent: Arc<Agent>) -> Arc<Self> {
        Arc::new(Self { agent })
    }

    pub async fn start(self: Arc<Self>, host: String, port: u16) -> Result<()> {
        let addr = format!("{host}:{port}");
        tracing::info!(stage = "server", status = "listening", addr = %addr);

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| EvotError::Run(format!("failed to bind {addr}: {e}")))?;

        axum::serve(listener, self.router())
            .await
            .map_err(|e| EvotError::Run(format!("server error: {e}")))?;

        Ok(())
    }

    fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route(
                "/",
                get(|State(server): State<Arc<Server>>| async move { server.index().await }),
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

    async fn chat(self: Arc<Self>, req: ChatRequest) -> impl IntoResponse {
        let stream = self.chat_stream(req.message, req.session_id);
        Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("ping"),
        )
    }

    fn chat_stream(
        self: Arc<Self>,
        message: String,
        session_id: Option<String>,
    ) -> impl futures::stream::Stream<
        Item = std::result::Result<axum::response::sse::Event, std::convert::Infallible>,
    > {
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            let request = QueryRequest::text(message).session_id(session_id);

            match self.agent.query(request).await {
                Ok(mut query_stream) => {
                    while let Some(event) = query_stream.next().await {
                        for sse in stream::map_run_event(&event) {
                            if tx.send(sse).await.is_err() {
                                break;
                            }
                        }
                    }
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
