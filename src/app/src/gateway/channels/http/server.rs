use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::response::Sse;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use parking_lot::RwLock;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use crate::agent::Agent;
use crate::agent::QueryRequest;
use crate::agent::SubmitOutcome;
use crate::conf::Config;
use crate::conf::FeishuSettings;
use crate::conf::ModelSettings;
use crate::error::EvotError;
use crate::error::Result;
use crate::gateway::channels::http::stream;

const INDEX_HTML: &str = include_str!("static/index.html");

// Shared console shell, plus one document per page. Served from the binary so
// the console works from any working directory.
const UI_CSS: &str = include_str!("static/ui/app.css");
const UI_JS: &str = include_str!("static/ui/app.js");
const MODELS_HTML: &str = include_str!("static/ui/models.html");
const MODELS_JS: &str = include_str!("static/ui/models.js");
const FEISHU_HTML: &str = include_str!("static/ui/feishu.html");
const FEISHU_JS: &str = include_str!("static/ui/feishu.js");
const CHAT_CSS: &str = include_str!("static/ui/chat.css");
const CHROME_JS: &str = include_str!("static/ui/chrome.js");

/// Static asset responses. Explicit content types: the router serves these from
/// memory, so nothing else infers one from a file extension. `no-cache` lets a
/// reload pick up a rebuilt binary's assets instead of pairing new markup with a
/// stale cached script.
fn css(body: &'static str) -> impl IntoResponse {
    (
        [
            ("content-type", "text/css; charset=utf-8"),
            ("cache-control", "no-cache"),
        ],
        body,
    )
}

fn js(body: &'static str) -> impl IntoResponse {
    (
        [
            ("content-type", "text/javascript; charset=utf-8"),
            ("cache-control", "no-cache"),
        ],
        body,
    )
}

/// `0` means no limit in the storage layer. The dashboard owns pagination, so
/// `/api/sessions` should return every saved session rather than an arbitrary
/// first page.
const SESSION_SEARCH_LIMIT: usize = 0;

/// Cap on ids accepted per `/api/sessions/delete` call. Bounds the work a single
/// request can trigger; the UI never selects more than the listed pool anyway.
const MAX_DELETE_BATCH: usize = 200;

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct DeleteSessionsRequest {
    ids: Vec<String>,
}

#[derive(Deserialize)]
struct ToggleFavoriteRequest {
    id: String,
}

pub struct Server {
    agent: Arc<Agent>,
    /// The live, mutable runtime config. Shared so the settings API can read a
    /// masked snapshot and apply edits in place, then persist to the env file.
    config: Arc<RwLock<Config>>,
}

impl Server {
    pub fn new(agent: Arc<Agent>, config: Config) -> Arc<Self> {
        Arc::new(Self {
            agent,
            config: Arc::new(RwLock::new(config)),
        })
    }

    pub async fn start(self: Arc<Self>, host: String, port: u16) -> Result<()> {
        let addr = format!("{host}:{port}");
        tracing::info!(stage = "server", status = "listening", addr = %addr);

        // Open the console in a browser once the server is up.
        let url = format!("http://{addr}/");
        let _ = std::thread::spawn(move || {
            // Small delay to ensure server is ready
            std::thread::sleep(std::time::Duration::from_millis(300));
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open").arg(&url).spawn();
            }
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            }
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("cmd")
                    .args(["/C", "start", &url])
                    .spawn();
            }
        });

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| EvotError::Run(format!("failed to bind {addr}: {e}")))?;

        axum::serve(listener, self.router())
            .await
            .map_err(|e| EvotError::Run(format!("server error: {e}")))?;

        Ok(())
    }

    pub fn router(self: Arc<Self>) -> Router {
        let dashboard = super::dashboard::dashboard_router(self.agent.clone());
        Router::new()
            .route(
                "/chat",
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
            .route(
                "/api/sessions",
                get(|State(server): State<Arc<Server>>| async move { server.sessions().await }),
            )
            .route(
                "/api/sessions/delete",
                post(
                    |State(server): State<Arc<Server>>,
                     Json(req): Json<DeleteSessionsRequest>| async move {
                        server.delete_sessions(req).await
                    },
                ),
            )
            .route(
                "/api/favorites",
                get(|State(server): State<Arc<Server>>| async move { server.favorites().await }),
            )
            .route(
                "/api/favorites/toggle",
                post(
                    |State(server): State<Arc<Server>>,
                     Json(req): Json<ToggleFavoriteRequest>| async move {
                        server.toggle_favorite(req).await
                    },
                ),
            )
            .route(
                "/api/models",
                get(|State(server): State<Arc<Server>>| async move { server.get_models() }).post(
                    |State(server): State<Arc<Server>>,
                     Json(req): Json<ModelSettings>| async move {
                        server.update_models(req)
                    },
                ),
            )
            .route(
                "/api/channels/feishu",
                get(|State(server): State<Arc<Server>>| async move { server.get_feishu() }).post(
                    |State(server): State<Arc<Server>>,
                     Json(req): Json<FeishuSettings>| async move {
                        server.update_feishu(req)
                    },
                ),
            )
            .route("/models", get(|| async { Html(MODELS_HTML) }))
            .route("/feishu", get(|| async { Html(FEISHU_HTML) }))
            .route("/ui/app.css", get(|| async { css(UI_CSS) }))
            .route("/ui/chat.css", get(|| async { css(CHAT_CSS) }))
            .route("/ui/app.js", get(|| async { js(UI_JS) }))
            .route("/ui/chrome.js", get(|| async { js(CHROME_JS) }))
            .route("/ui/models.js", get(|| async { js(MODELS_JS) }))
            .route("/ui/feishu.js", get(|| async { js(FEISHU_JS) }))
            // Kept so existing links and bookmarks still land somewhere useful
            // now that provider and channel config have their own pages.
            .route("/settings", get(|| async { Redirect::permanent("/models") }))
            .with_state(self)
            .merge(dashboard)
            .layer(CorsLayer::permissive())
    }

    async fn index(&self) -> Html<&'static str> {
        Html(INDEX_HTML)
    }

    /// Provider + model state with secrets masked, for the Models page.
    ///
    /// Reloads from the env file on disk first, so the page reflects edits made
    /// outside the dashboard (e.g. a hand-edited `evot.env`) rather than a stale
    /// in-memory snapshot. This also keeps the next save's "leave blank to keep"
    /// behavior anchored to the real on-disk secrets.
    fn get_models(&self) -> impl IntoResponse {
        if let Err(e) = self.reload_config_from_disk() {
            // Fall back to the in-memory config rather than failing the page; a
            // transient read error shouldn't blank out the page.
            tracing::warn!("models: reload from disk failed, serving cached config: {e}");
        }
        Json(crate::conf::models_snapshot(&self.config.read()))
    }

    /// Feishu channel state with the secret masked, for the Feishu page.
    fn get_feishu(&self) -> impl IntoResponse {
        if let Err(e) = self.reload_config_from_disk() {
            tracing::warn!("feishu: reload from disk failed, serving cached config: {e}");
        }
        Json(crate::conf::feishu_snapshot(&self.config.read()))
    }

    /// Re-read the env file from disk and replace the shared config. Uses the
    /// path the process was started with so a custom `--env-file` is honored.
    fn reload_config_from_disk(&self) -> Result<()> {
        let env_path = self.config.read().env_file_path.clone();
        let path_arg = env_path.to_str();
        let fresh = Config::load_with_env_file(path_arg)?;
        *self.config.write() = fresh;
        Ok(())
    }

    /// Validate, persist, and hot-apply a provider/model update. Takes effect on
    /// the next message: the agent's `LlmConfig` is rebuilt here.
    fn update_models(&self, update: ModelSettings) -> impl IntoResponse {
        match self
            .apply_and_persist(|candidate| crate::conf::apply_model_settings(candidate, &update))
        {
            Ok(()) => (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "models": crate::conf::models_snapshot(&self.config.read()),
                })),
            )
                .into_response(),
            Err(e) => (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
            )
                .into_response(),
        }
    }

    /// Validate, persist, and apply a Feishu channel update. Persisting is
    /// enough for the config, but the channel is spawned at startup, so the
    /// response reports that a restart is needed to pick the change up.
    fn update_feishu(&self, update: FeishuSettings) -> impl IntoResponse {
        match self
            .apply_and_persist(|candidate| crate::conf::apply_feishu_settings(candidate, &update))
        {
            Ok(()) => (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "restart_required": true,
                    "channel": crate::conf::feishu_snapshot(&self.config.read()),
                })),
            )
                .into_response(),
            Err(e) => (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
            )
                .into_response(),
        }
    }

    /// Apply `mutate` to a copy of the shared config, persist the result to the
    /// env file, and push the rebuilt active LLM into the running agent. Holds
    /// the write lock for the whole operation so concurrent edits from separate
    /// pages cannot interleave and lose each other's changes.
    fn apply_and_persist(&self, mutate: impl FnOnce(&mut Config) -> Result<()>) -> Result<()> {
        let mut config = self.config.write();
        let mut candidate = config.clone();
        mutate(&mut candidate)?;
        // Surface resolution errors (e.g. missing key) before writing the file.
        let llm = candidate.active_llm()?;
        let env_path = candidate.env_file_path.clone();
        // Persist before publishing the candidate so a write failure leaves the
        // live config and running agent on the last durable configuration.
        let groups = crate::conf::config_to_env_groups(&candidate);
        crate::conf::env_writer::write_grouped(&env_path, &groups)?;
        *config = candidate;
        self.agent.set_llm(llm);
        Ok(())
    }

    /// Returns recent sessions, each with a flattened `search_text` field
    /// (id, title, cwd, model plus transcript snippets) so the chat UI can do
    /// client-side substring filtering and highlight matches, mirroring the
    /// terminal `/resume` selector.
    async fn sessions(&self) -> impl IntoResponse {
        match self
            .agent
            .list_sessions_with_text(SESSION_SEARCH_LIMIT)
            .await
        {
            Ok(sessions) => Json(sessions).into_response(),
            Err(e) => {
                tracing::warn!("chat: failed to list sessions with text: {e}");
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to list sessions",
                )
                    .into_response()
            }
        }
    }

    /// Deletes the given sessions and reports how many were removed. Invalid or
    /// already-gone ids are skipped rather than failing the whole batch, so a
    /// concurrent deletion or a stale client list cannot wedge the request. Any
    /// active run for a session is aborted first so it cannot re-create the
    /// session directory and leave zombie state behind.
    async fn delete_sessions(&self, req: DeleteSessionsRequest) -> impl IntoResponse {
        if req.ids.len() > MAX_DELETE_BATCH {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("too many ids: {} (max {MAX_DELETE_BATCH})", req.ids.len()),
                })),
            )
                .into_response();
        }
        let mut deleted = 0usize;
        let mut deleted_ids: Vec<String> = Vec::new();
        let mut failed: Vec<String> = Vec::new();
        for id in &req.ids {
            // Stop an in-flight run before removing files, otherwise the run's
            // next transcript write would recreate the directory.
            self.agent.abort_run(id);
            match self.agent.delete_session(id).await {
                Ok(true) => {
                    deleted += 1;
                    deleted_ids.push(id.clone());
                }
                Ok(false) => {
                    // Already gone: still prune a stale favorite entry for this id.
                    deleted_ids.push(id.clone());
                }
                Err(e) => {
                    tracing::warn!(session_id = %id, "delete failed: {e}");
                    failed.push(id.clone());
                }
            }
        }
        if !deleted_ids.is_empty() {
            if let Err(e) = self.agent.remove_favorites(&deleted_ids).await {
                tracing::warn!("failed to prune deleted sessions from favorites: {e}");
            }
        }
        Json(serde_json::json!({
            "deleted": deleted,
            "requested": req.ids.len(),
            "failed": failed,
        }))
        .into_response()
    }

    /// Returns the set of favorited session ids so the dashboard can pin and
    /// sort them on load.
    async fn favorites(&self) -> impl IntoResponse {
        match self.agent.list_favorites().await {
            Ok(ids) => Json(serde_json::json!({ "ids": ids })).into_response(),
            Err(e) => {
                tracing::warn!("dashboard: failed to list favorites: {e}");
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to list favorites",
                )
                    .into_response()
            }
        }
    }

    /// Toggles a session's favorite state and reports the new value.
    async fn toggle_favorite(&self, req: ToggleFavoriteRequest) -> impl IntoResponse {
        match self.agent.toggle_favorite(&req.id).await {
            Ok(favorited) => {
                Json(serde_json::json!({ "id": req.id, "favorited": favorited })).into_response()
            }
            Err(e) => {
                tracing::warn!(session_id = %req.id, "toggle favorite failed: {e}");
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to toggle favorite",
                )
                    .into_response()
            }
        }
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
            let request = QueryRequest::text(&message)
                .session_id(session_id)
                .source("http");

            let drain_run = |mut query_run: crate::agent::Run, tx: tokio::sync::mpsc::Sender<_>| async move {
                while let Some(event) = query_run.next().await {
                    for sse in stream::map_run_event(&event) {
                        if tx.send(sse).await.is_err() {
                            break;
                        }
                    }
                }
            };

            match self.agent.submit(request).await {
                Ok(SubmitOutcome::Run(query_run)) => {
                    drain_run(query_run, tx.clone()).await;
                }
                Ok(SubmitOutcome::Command(text)) => {
                    let _ = tx.send(stream::text_event(&text)).await;
                }
                Err(e) => {
                    let _ = tx.send(stream::error_event(e.to_string())).await;
                }
            }

            let _ = tx.send(stream::done_event()).await;
        });

        tokio_stream::wrappers::ReceiverStream::new(rx)
    }
}
