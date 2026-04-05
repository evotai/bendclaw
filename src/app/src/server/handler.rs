use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Sse;
use axum::Json;
use futures::stream::Stream;
use serde::Deserialize;
use serde::Serialize;
use tokio_stream::wrappers::ReceiverStream;

use crate::run;
use crate::run::RunRequest;
use crate::server::server::AppState;
use crate::server::stream;

const INDEX_HTML: &str = include_str!("static/index.html");

#[derive(Deserialize)]
pub(crate) struct ChatRequest {
    message: String,
}

#[derive(Serialize)]
pub(crate) struct StatusResponse {
    status: String,
}

pub(crate) async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub(crate) async fn new_session(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    *state.session_id.lock().await = None;
    Json(StatusResponse {
        status: "ok".into(),
    })
}

pub(crate) async fn chat(
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
        let current_session_id = state.session_id.lock().await.clone();
        let sink = stream::SseSink::new(tx.clone());
        let mut request = RunRequest::new(message);
        request.session_id = current_session_id;

        match run::run(request, state.llm.clone(), &sink, state.storage.as_ref()).await {
            Ok(output) => {
                *state.session_id.lock().await = Some(output.session_id);
            }
            Err(error) => {
                let _ = tx.send(stream::error_event(error.to_string())).await;
            }
        }

        let _ = tx.send(stream::done_event()).await;
    });

    ReceiverStream::new(rx)
}
