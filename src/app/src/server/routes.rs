use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Sse;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use futures::stream::Stream;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;

use crate::agent::build_agent_options;
use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;
use crate::server::stream;

const INDEX_HTML: &str = include_str!("index.html");

struct AppState {
    agent: Mutex<Option<bend_agent::Agent>>,
    llm: LlmConfig,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
}

pub async fn start(conf: Config) -> Result<()> {
    let llm = conf.active_llm();
    let state = Arc::new(AppState {
        agent: Mutex::new(None),
        llm,
    });

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/new", post(new_session_handler))
        .route("/api/chat", post(chat_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", conf.server.host, conf.server.port);
    tracing::info!("bendclaw server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| BendclawError::Run(format!("failed to bind {addr}: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| BendclawError::Run(format!("server error: {e}")))?;

    Ok(())
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn new_session_handler(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    if let Some(agent) = state.agent.lock().await.take() {
        agent.close().await;
    }
    Json(StatusResponse {
        status: "ok".into(),
    })
}

async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let stream = chat_stream(state, req.message);
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    )
}

fn chat_stream(
    state: Arc<AppState>,
    message: String,
) -> impl Stream<Item = std::result::Result<axum::response::sse::Event, Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel(64);

    tokio::spawn(async move {
        let start = std::time::Instant::now();

        let mut agent = state.agent.lock().await.take();

        if agent.is_none() {
            let opts = build_agent_options(&state.llm, None, Some(20));
            match bend_agent::Agent::new(opts).await {
                Ok(a) => agent = Some(a),
                Err(e) => {
                    let _ = tx.send(stream::error_event(e.to_string())).await;
                    let _ = tx.send(stream::done_event()).await;
                    return;
                }
            }
        }

        let mut agent = match agent {
            Some(a) => a,
            None => return,
        };

        let (mut sdk_rx, handle) = agent.query(&message).await;

        while let Some(event) = sdk_rx.recv().await {
            let sse_data = stream::map_sdk_message(&event, &start);
            for data in sse_data {
                if tx.send(data).await.is_err() {
                    break;
                }
            }
        }

        let _ = handle.await;
        let _ = tx.send(stream::done_event()).await;

        *state.agent.lock().await = Some(agent);
    });

    ReceiverStream::new(rx)
}
