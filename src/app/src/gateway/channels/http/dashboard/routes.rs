use std::sync::Arc;

use axum::extract::Path;
use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use axum::Router;

use super::trace;
use crate::agent::Agent;
use crate::types::ListTranscriptEntries;

// Session list + trace pages. Both are plain documents; the console shell and
// its shared assets are served by the http server module.
const SESSIONS_HTML: &str = include_str!("../static/ui/sessions.html");
const SESSIONS_JS: &str = include_str!("../static/ui/sessions.js");

#[derive(Clone)]
pub struct DashboardState {
    pub agent: Arc<Agent>,
}

pub fn dashboard_router(agent: Arc<Agent>) -> Router {
    let state = DashboardState { agent };
    Router::new()
        .route("/ui/sessions.js", get(sessions_js))
        .route("/api/vitals", get(api_vitals))
        // Session trace (per-LLM-call spans with tool calls)
        .route("/api/session/{id}/events", get(api_events))
        .route("/api/session/{id}/events/{seq}", get(api_event_detail))
        .route("/api/session/{id}/activity", get(api_activity))
        // Session list, and the trace viewer for one session
        .route("/", get(sessions_page))
        .route("/sessions", get(sessions_page))
        .route("/sessions/{id}", get(trace_page))
        .route("/sessions/{id}/trace", get(trace_page))
        .with_state(state)
}

async fn sessions_page() -> Html<&'static str> {
    Html(SESSIONS_HTML)
}

async fn sessions_js() -> impl IntoResponse {
    (
        [
            ("content-type", "text/javascript; charset=utf-8"),
            ("cache-control", "no-cache"),
        ],
        SESSIONS_JS,
    )
}

// --- API: host vitals ---

/// CPU, memory, and disk for the Sessions page header.
///
/// This was a 2s WebSocket push feeding the old SPA. The console reads it once
/// per page load instead: the numbers are coarse gauges, and a socket that
/// re-rendered the whole session grid every two seconds cost more than it told
/// anyone.
async fn api_vitals() -> impl IntoResponse {
    let m = super::metrics::collect();
    Json(serde_json::json!({
        "cpu_percent": m.cpu_percent,
        "cpu_available": m.cpu_available,
        "ram_used": (m.ram_used_mb * 1_048_576.0) as u64,
        "ram_total": (m.ram_total_mb * 1_048_576.0) as u64,
        "ram_percent": m.ram_percent,
        "disk_total": m.disk_total_gb * 1_073_741_824,
        "disk_used": m.disk_total_gb.saturating_sub(m.disk_available_gb) * 1_073_741_824,
        "disk_percent": m.disk_percent,
    }))
}

// --- API: session trace (per-LLM-call spans) ---

const TRACE_HTML: &str = include_str!("../static/trace/index.html");

async fn load_entries(state: &DashboardState, id: &str) -> Vec<crate::types::TranscriptEntry> {
    state
        .agent
        .storage()
        .list_entries(ListTranscriptEntries {
            session_id: id.to_string(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await
        .unwrap_or_default()
}

async fn api_events(
    State(state): State<DashboardState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let entries = load_entries(&state, &id).await;
    Json(trace::project_spans(&entries))
}

async fn api_event_detail(
    State(state): State<DashboardState>,
    Path((id, seq)): Path<(String, u64)>,
) -> impl IntoResponse {
    let entries = load_entries(&state, &id).await;
    match trace::project_span_detail(&entries, seq) {
        Some(detail) => Json(detail).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "span not found").into_response(),
    }
}

async fn api_activity(
    State(state): State<DashboardState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let meta = state.agent.storage().get_session(&id).await.ok().flatten();
    let entries = load_entries(&state, &id).await;
    Json(trace::project_activity(&entries, meta.as_ref())).into_response()
}

async fn trace_page() -> Html<&'static str> {
    Html(TRACE_HTML)
}
