use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("rate limited: {0}")]
    RateLimited(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<crate::base::ErrorCode> for ServiceError {
    fn from(e: crate::base::ErrorCode) -> Self {
        use crate::base::ErrorCode;

        match e.code {
            ErrorCode::NOT_FOUND | ErrorCode::SKILL_NOT_FOUND => {
                tracing::warn!(code = e.code, name = e.name, error = %e, "resource not found");
                Self::AgentNotFound(e.message)
            }
            ErrorCode::INVALID_INPUT => {
                tracing::warn!(code = e.code, name = e.name, error = %e, "bad request");
                Self::BadRequest(e.message)
            }
            ErrorCode::DENIED => {
                tracing::warn!(code = e.code, name = e.name, error = %e, "forbidden");
                Self::Forbidden(e.message)
            }
            ErrorCode::LLM_RATE_LIMIT | ErrorCode::QUOTA_EXCEEDED => {
                tracing::warn!(code = e.code, name = e.name, error = %e, "rate limited");
                Self::RateLimited(e.message)
            }
            _ => {
                let display = format!("[{}] {}: {}", e.code, e.name, e.message);
                tracing::error!(
                    code = e.code,
                    name = e.name,
                    error = %e,
                    span_trace = %tracing_error::SpanTrace::capture(),
                    "internal error"
                );
                Self::Internal(display)
            }
        }
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            Self::AgentNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            Self::BadRequest(m) => {
                tracing::warn!(error = %m, "bad request");
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, self.to_string()),
            Self::Conflict(_) => (StatusCode::CONFLICT, self.to_string()),
            Self::RateLimited(_) => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

pub type Result<T> = std::result::Result<T, ServiceError>;
