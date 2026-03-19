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
        if s.len() >= 2 { format!("...{}", &s[s.len()-2..]) } else { "***".to_string() }
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

fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

fn extract_query_param(uri: &axum::http::Uri, key: &str) -> Option<String> {
    uri.query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let name = parts.next().unwrap_or_default();
            let value = parts.next().unwrap_or_default();
            (name == key && !value.is_empty()).then(|| value.to_string())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_bearer_valid() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer mykey".parse().unwrap(),
        );
        assert_eq!(extract_bearer(&headers), Some("mykey".to_string()));
    }

    #[test]
    fn extract_bearer_missing() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(extract_bearer(&headers), None);
    }

    #[test]
    fn extract_bearer_wrong_scheme() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Basic abc".parse().unwrap(),
        );
        assert_eq!(extract_bearer(&headers), None);
    }

    #[test]
    fn extract_query_param_present() {
        let uri: axum::http::Uri = "/health?api_key=secret123".parse().unwrap();
        assert_eq!(
            extract_query_param(&uri, "api_key"),
            Some("secret123".to_string())
        );
    }

    #[test]
    fn extract_query_param_missing() {
        let uri: axum::http::Uri = "/health?other=val".parse().unwrap();
        assert_eq!(extract_query_param(&uri, "api_key"), None);
    }

    #[test]
    fn extract_query_param_empty_value() {
        let uri: axum::http::Uri = "/health?api_key=".parse().unwrap();
        assert_eq!(extract_query_param(&uri, "api_key"), None);
    }
}
