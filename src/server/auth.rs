use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;

use super::state::AppState;

/// Auth middleware: validates `Authorization: Bearer <key>` header.
/// For SSE-friendly endpoints, also accepts `?api_key=<key>` query param.
pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let expected = &state.auth_key;
    if expected.is_empty() {
        return next.run(req).await;
    }

    let token = extract_bearer(req.headers()).or_else(|| extract_query_param(req.uri(), "api_key"));

    let suffix = |s: &str| -> String {
        if s.len() >= 2 {
            format!("...{}", &s[s.len() - 2..])
        } else {
            "***".to_string()
        }
    };

    match token {
        Some(t) if t == *expected => next.run(req).await,
        Some(t) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": format!(
                    "API key mismatch: AgentOS expects key ending with \"{}\", but received key ending with \"{}\"",
                    suffix(expected),
                    suffix(&t)
                )
            })),
        )
            .into_response(),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": format!(
                    "Missing API key. AgentOS expects key ending with \"{}\"",
                    suffix(expected)
                )
            })),
        )
            .into_response(),
    }
}

pub fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

pub fn extract_query_param(uri: &axum::http::Uri, key: &str) -> Option<String> {
    uri.query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let name = parts.next().unwrap_or_default();
            let value = parts.next().unwrap_or_default();
            (name == key && !value.is_empty()).then(|| value.to_string())
        })
    })
}
