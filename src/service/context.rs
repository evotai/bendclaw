use axum::extract::FromRequestParts;
use axum::extract::Query;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

const USER_HEADER: &str = "x-user-id";
const TRACE_HEADER: &str = "x-request-id";

/// Per-request context extracted from headers.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub user_id: String,
    pub trace_id: String,
}

pub struct MissingHeader(&'static str);

impl IntoResponse for MissingHeader {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": format!("missing required header: {}", self.0) })),
        )
            .into_response()
    }
}

impl RequestContext {
    fn from_parts(parts: &Parts) -> Result<Self, MissingHeader> {
        #[derive(Deserialize)]
        struct QueryParams {
            #[serde(rename = "x-user-id")]
            user_id: Option<String>,
        }

        let header = |key: &str| {
            parts
                .headers
                .get(key)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        };

        // Try header first, fall back to query param (needed for EventSource/SSE)
        let user_id = header(USER_HEADER)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                Query::<QueryParams>::try_from_uri(&parts.uri)
                    .ok()
                    .and_then(|q| q.0.user_id)
                    .filter(|s| !s.is_empty())
            })
            .ok_or(MissingHeader(USER_HEADER))?;

        Ok(Self {
            user_id,
            trace_id: header(TRACE_HEADER).unwrap_or_default(),
        })
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestContext {
    type Rejection = MissingHeader;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Self::from_parts(parts)
    }
}
