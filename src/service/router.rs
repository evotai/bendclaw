use std::time::Instant;

use axum::body::Body;
use axum::extract::MatchedPath;
use axum::http::Request;
use axum::middleware;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::delete;
use axum::routing::get;
use axum::routing::post;
use axum::routing::put;
use axum::Router;
use tower_http::cors::AllowOrigin;
use tower_http::cors::Any;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::auth::auth_middleware;
use super::health::health_check;
use super::routes;
use super::state::AppState;
use super::v1;
use crate::config::AuthConfig;

const TRACE_HEADER: &str = "x-request-id";
const USER_HEADER: &str = "x-user-id";

fn header_value(req: &Request<Body>, key: &str) -> String {
    req.headers()
        .get(key)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn query_value(req: &Request<Body>, key: &str) -> String {
    req.uri()
        .query()
        .and_then(|query| {
            query.split('&').find_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let name = parts.next().unwrap_or_default();
                let value = parts.next().unwrap_or_default();
                (name == key).then(|| value.replace('+', " "))
            })
        })
        .unwrap_or_default()
}

async fn log_http_request(req: Request<Body>, next: Next) -> Response {
    let started = Instant::now();
    let method = req.method().clone();
    let uri = req.uri().clone();
    let matched_path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_string())
        .unwrap_or_default();
    let trace_id = header_value(&req, TRACE_HEADER);
    let user_id = {
        let header = header_value(&req, USER_HEADER);
        if header.is_empty() {
            query_value(&req, USER_HEADER)
        } else {
            header
        }
    };
    let user_agent = header_value(&req, "user-agent");
    let content_length = req
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let forwarded_for = header_value(&req, "x-forwarded-for");
    let real_ip = header_value(&req, "x-real-ip");
    let query = uri.query().unwrap_or_default().to_string();

    tracing::info!(
        log_kind = "server_log",
        stage = "http",
        status = "received",
        method = %method,
        uri = %uri,
        matched_path,
        trace_id,
        user_id,
        user_agent,
        content_length,
        forwarded_for,
        real_ip,
        query,
        "http request"
    );

    let response = next.run(req).await;
    let status = response.status();
    let elapsed_ms = started.elapsed().as_millis() as u64;

    if status.is_server_error() {
        tracing::error!(
            log_kind = "server_log",
            stage = "http",
            status = "completed",
            method = %method,
            uri = %uri,
            matched_path,
            trace_id,
            user_id,
            user_agent,
            content_length,
            forwarded_for,
            real_ip,
            query,
            response_status = status.as_u16(),
            elapsed_ms,
            "http request"
        );
    } else {
        tracing::info!(
            log_kind = "server_log",
            stage = "http",
            status = "completed",
            method = %method,
            uri = %uri,
            matched_path,
            trace_id,
            user_id,
            user_agent,
            content_length,
            forwarded_for,
            real_ip,
            query,
            response_status = status.as_u16(),
            elapsed_ms,
            "http request"
        );
    }

    response
}

fn build_cors(auth: &AuthConfig) -> CorsLayer {
    if auth.is_enabled() {
        let origins: Vec<axum::http::HeaderValue> = auth
            .allowed_origins()
            .into_iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    }
}

pub fn api_router(state: AppState, _log_level: &str, auth: &AuthConfig) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/v1/agents/{agent_id}/setup", post(routes::setup_agent))
        .route("/v1/stats/sessions", get(routes::session_stats))
        .route("/v1/stats/can_suspend", get(routes::can_suspend))
        // Agents
        .route("/v1/agents", get(v1::agents::list_agents))
        .route(
            "/v1/agents/{agent_id}",
            get(v1::agents::get_agent).delete(v1::agents::delete_agent),
        )
        // Sessions
        .route(
            "/v1/agents/{agent_id}/sessions",
            get(v1::sessions::list_sessions).post(v1::sessions::create_session),
        )
        .route(
            "/v1/agents/{agent_id}/sessions/{session_id}",
            get(v1::sessions::get_session)
                .put(v1::sessions::update_session)
                .delete(v1::sessions::delete_session),
        )
        // Runs
        .route(
            "/v1/agents/{agent_id}/sessions/{session_id}/runs",
            get(v1::runs::list_runs_by_session),
        )
        .route(
            "/v1/agents/{agent_id}/runs/{run_id}",
            get(v1::runs::get_run),
        )
        .route(
            "/v1/agents/{agent_id}/runs",
            get(v1::runs::list_runs).post(v1::runs::create_run),
        )
        .route(
            "/v1/agents/{agent_id}/runs/{run_id}/cancel",
            post(v1::runs::cancel_run),
        )
        .route(
            "/v1/agents/{agent_id}/runs/{run_id}/continue",
            post(v1::runs::continue_run),
        )
        // Memory
        .route(
            "/v1/agents/{agent_id}/memories",
            get(v1::memories::list_memories).post(v1::memories::create_memory),
        )
        .route(
            "/v1/agents/{agent_id}/memories/{memory_id}",
            get(v1::memories::get_memory).delete(v1::memories::delete_memory),
        )
        .route(
            "/v1/agents/{agent_id}/memories/search",
            post(v1::memories::search_memories),
        )
        // Skills
        .route(
            "/v1/agents/{agent_id}/skills",
            get(v1::skills::list_skills).post(v1::skills::create_skill),
        )
        .route(
            "/v1/agents/{agent_id}/skills/{skill_name}",
            get(v1::skills::get_skill).delete(v1::skills::delete_skill),
        )
        // Config
        .route(
            "/v1/agents/{agent_id}/config",
            get(v1::config::get_config).put(v1::config::update_config),
        )
        .route(
            "/v1/agents/{agent_id}/config/versions",
            get(v1::config::list_versions),
        )
        .route(
            "/v1/agents/{agent_id}/config/versions/{version}",
            get(v1::config::get_version),
        )
        .route(
            "/v1/agents/{agent_id}/config/rollback",
            post(v1::config::rollback_config),
        )
        // Learnings
        .route(
            "/v1/agents/{agent_id}/learnings",
            get(v1::learnings::list_learnings).post(v1::learnings::create_learning),
        )
        .route(
            "/v1/agents/{agent_id}/learnings/{learning_id}",
            delete(v1::learnings::delete_learning),
        )
        .route(
            "/v1/agents/{agent_id}/learnings/search",
            post(v1::learnings::search_learnings),
        )
        // Traces
        .route("/v1/agents/{agent_id}/traces", get(v1::traces::list_traces))
        .route(
            "/v1/agents/{agent_id}/traces/summary",
            get(v1::traces::traces_summary),
        )
        .route(
            "/v1/agents/{agent_id}/traces/{trace_id}",
            get(v1::traces::get_trace),
        )
        .route(
            "/v1/agents/{agent_id}/traces/{trace_id}/spans",
            get(v1::traces::list_spans),
        )
        // Run Events
        .route(
            "/v1/agents/{agent_id}/runs/{run_id}/events",
            get(v1::runs::list_run_events),
        )
        // Usage
        .route("/v1/agents/{agent_id}/usage", get(v1::usage::usage_summary))
        .route(
            "/v1/agents/{agent_id}/usage/daily",
            get(v1::usage::usage_daily),
        )
        .route("/v1/usage/summary", get(v1::usage::global_usage_summary))
        // Variables
        .route(
            "/v1/agents/{agent_id}/variables",
            get(v1::variables::list_variables).post(v1::variables::create_variable),
        )
        .route(
            "/v1/agents/{agent_id}/variables/{var_id}",
            get(v1::variables::get_variable)
                .put(v1::variables::update_variable)
                .delete(v1::variables::delete_variable),
        )
        // Tasks
        .route(
            "/v1/agents/{agent_id}/tasks",
            get(v1::tasks::list_tasks).post(v1::tasks::create_task),
        )
        .route(
            "/v1/agents/{agent_id}/tasks/{task_id}",
            put(v1::tasks::update_task).delete(v1::tasks::delete_task),
        )
        .route(
            "/v1/agents/{agent_id}/tasks/{task_id}/toggle",
            post(v1::tasks::toggle_task),
        )
        .route(
            "/v1/agents/{agent_id}/tasks/{task_id}/history",
            get(v1::tasks::list_task_history),
        )
        // Feedback
        .route(
            "/v1/agents/{agent_id}/feedback",
            get(v1::feedback::list_feedback).post(v1::feedback::create_feedback),
        )
        .route(
            "/v1/agents/{agent_id}/feedback/{feedback_id}",
            delete(v1::feedback::delete_feedback),
        )
        .layer(build_cors(auth))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(middleware::from_fn(log_http_request))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
