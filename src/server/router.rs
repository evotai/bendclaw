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
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::auth::auth_middleware;
use super::health::health_check;
use super::routes;
use super::state::AppState;
use super::v1;
use crate::config::AuthConfig;
use crate::observability::log::slog;

const TRACE_HEADER: &str = "x-request-id";
const USER_HEADER: &str = "x-user-id";

/// Resolved request trace ID, injected by the logging middleware.
#[derive(Clone)]
pub struct ResolvedTraceId(pub String);

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

async fn log_http_request(mut req: Request<Body>, next: Next) -> Response {
    let started = Instant::now();
    let method = req.method().clone();
    let uri = req.uri().clone();
    let matched_path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_string())
        .unwrap_or_default();

    // Skip logging for health checks and CORS preflight to reduce noise.
    if matched_path == "/health" || method == axum::http::Method::OPTIONS {
        let mut response = next.run(req).await;
        set_private_network_header(&mut response);
        return response;
    }

    let trace_id = {
        let header = header_value(&req, TRACE_HEADER);
        if header.is_empty() {
            ulid::Ulid::new().to_string().to_lowercase()
        } else {
            header
        }
    };
    req.extensions_mut()
        .insert(ResolvedTraceId(trace_id.clone()));
    let user_id = {
        let header = header_value(&req, USER_HEADER);
        if header.is_empty() {
            query_value(&req, USER_HEADER)
        } else {
            header
        }
    };
    let content_length = req
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);

    let mut response = next.run(req).await;
    set_private_network_header(&mut response);
    let status = response.status();
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let path = if matched_path.is_empty() {
        uri.path().to_string()
    } else {
        matched_path
    };
    let code = status.as_u16();

    if status.is_server_error() {
        slog!(error, "service", "http_request", code, method = %method, path = %path, elapsed_ms, content_length, trace_id = %trace_id, user_id = %user_id,);
    } else if status.is_client_error() {
        slog!(warn, "service", "http_request", code, method = %method, path = %path, elapsed_ms, content_length, trace_id = %trace_id, user_id = %user_id,);
    }

    response
}

/// Chrome 104+ requires this header on preflight responses to allow
/// public-origin pages to access private-network (localhost) endpoints.
fn set_private_network_header(response: &mut Response) {
    response.headers_mut().insert(
        axum::http::header::HeaderName::from_static("access-control-allow-private-network"),
        axum::http::HeaderValue::from_static("true"),
    );
}

fn build_cors(auth: &AuthConfig) -> CorsLayer {
    // Always use an explicit origin list instead of `*`.
    // Chrome's Private Network Access preflights are always credentialed,
    // and CORS rejects `Access-Control-Allow-Origin: *` for credentialed requests.
    let origins: Vec<axum::http::HeaderValue> = auth
        .allowed_origins()
        .into_iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    use axum::http::header;
    let allowed_headers = [
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
        header::ACCEPT,
        header::ORIGIN,
        header::HeaderName::from_static("x-request-id"),
        header::HeaderName::from_static("x-user-id"),
    ];

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(allowed_headers)
        .allow_credentials(true)
}

pub fn api_router(state: AppState, _log_level: &str, auth: &AuthConfig) -> Router {
    let authenticated_routes = Router::new()
        .route("/v1/agents/{agent_id}/setup", post(routes::setup_agent))
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
        // Skills
        .route(
            "/v1/skills",
            get(v1::skills::list_skills).post(v1::skills::create_skill),
        )
        .route(
            "/v1/skills/{*skill_key}",
            get(v1::skills::get_skill).delete(v1::skills::delete_skill),
        )
        // Hub
        .route("/v1/hub/skills", get(v1::hub::list_hub_skills))
        .route(
            "/v1/hub/skills/{skill_name}/credentials",
            get(v1::hub::skill_credentials),
        )
        .route("/v1/hub/status", get(v1::hub::hub_status))
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
        .route(
            "/v1/agents/{agent_id}/traces/{trace_id}/children",
            get(v1::traces::list_child_traces),
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
        // Workbench
        .route(
            "/v1/agents/{agent_id}/workbench/sessions/{session_id}/replay",
            get(v1::workbench::replay::get_replay),
        )
        // System
        .route("/v1/system/upgrade", post(v1::system::upgrade))
        // Channels
        .route(
            "/v1/agents/{agent_id}/channels/accounts",
            get(v1::channels::list_accounts).post(v1::channels::create_account),
        )
        .route(
            "/v1/agents/{agent_id}/channels/accounts/{account_id}",
            get(v1::channels::get_account).delete(v1::channels::delete_account),
        )
        .route(
            "/v1/agents/{agent_id}/channels/messages",
            get(v1::channels::list_messages),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // Webhook routes — no auth (external platforms can't authenticate).
    let webhook_routes = Router::new().route(
        "/v1/agents/{agent_id}/channels/webhook/{account_id}",
        post(v1::channels::webhook),
    );

    let public_routes = Router::new().route("/health", get(health_check));

    public_routes
        .merge(webhook_routes)
        .merge(authenticated_routes)
        .layer(build_cors(auth))
        .layer(middleware::from_fn(log_http_request))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
