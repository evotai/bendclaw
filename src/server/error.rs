use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use crate::observability::log::slog;

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

    #[error("{message}")]
    Detailed {
        status: StatusCode,
        kind: &'static str,
        message: String,
        code: Option<u16>,
        name: Option<&'static str>,
        details: Option<ErrorDetails>,
    },
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorPayload,
}

#[derive(Debug, Serialize)]
struct ErrorPayload {
    status: u16,
    kind: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<ErrorDetails>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ErrorDetails {
    Storage(StorageErrorDetails),
}

#[derive(Debug, Serialize)]
pub struct StorageErrorDetails {
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upstream_message: Option<String>,
}

#[derive(Debug, Default)]
struct ParsedStorageError {
    operation: Option<String>,
    upstream_status: Option<u16>,
    upstream_error: Option<String>,
    upstream_message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct StorageProxyErrorBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    message: String,
}

impl From<crate::types::ErrorCode> for ServiceError {
    fn from(e: crate::types::ErrorCode) -> Self {
        use crate::types::ErrorCode;

        match e.code {
            ErrorCode::NOT_FOUND | ErrorCode::SKILL_NOT_FOUND => {
                slog!(warn, "service", "not_found", code = e.code, name = e.name, error = %e,);
                Self::Detailed {
                    status: StatusCode::NOT_FOUND,
                    kind: "not_found",
                    message: e.message,
                    code: Some(e.code),
                    name: Some(e.name),
                    details: None,
                }
            }
            ErrorCode::INVALID_INPUT
            | ErrorCode::SKILL_VALIDATION
            | ErrorCode::SKILL_REQUIREMENTS => {
                slog!(warn, "service", "bad_request", code = e.code, name = e.name, error = %e,);
                Self::Detailed {
                    status: StatusCode::BAD_REQUEST,
                    kind: "bad_request",
                    message: e.message,
                    code: Some(e.code),
                    name: Some(e.name),
                    details: None,
                }
            }
            ErrorCode::DENIED => {
                slog!(warn, "service", "forbidden", code = e.code, name = e.name, error = %e,);
                Self::Detailed {
                    status: StatusCode::FORBIDDEN,
                    kind: "forbidden",
                    message: e.message,
                    code: Some(e.code),
                    name: Some(e.name),
                    details: None,
                }
            }
            ErrorCode::LLM_RATE_LIMIT | ErrorCode::QUOTA_EXCEEDED => {
                slog!(warn, "service", "rate_limited", code = e.code, name = e.name, error = %e,);
                Self::Detailed {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    kind: "rate_limited",
                    message: e.message,
                    code: Some(e.code),
                    name: Some(e.name),
                    details: None,
                }
            }
            ErrorCode::STORAGE_CONNECTION
            | ErrorCode::STORAGE_EXEC
            | ErrorCode::STORAGE_GATEWAY => {
                let (status, kind, retryable) = match e.code {
                    ErrorCode::STORAGE_CONNECTION => {
                        (StatusCode::SERVICE_UNAVAILABLE, "storage_connection", true)
                    }
                    ErrorCode::STORAGE_GATEWAY => {
                        (StatusCode::BAD_GATEWAY, "storage_gateway", true)
                    }
                    _ => (StatusCode::INTERNAL_SERVER_ERROR, "storage_exec", false),
                };
                let parsed = parse_storage_error(&e.message);
                let message = parsed
                    .operation
                    .as_deref()
                    .map(|op| format!("storage {op} failed"))
                    .unwrap_or_else(|| "storage request failed".to_string());
                let details = ErrorDetails::Storage(StorageErrorDetails {
                    retryable,
                    operation: parsed.operation,
                    upstream_status: parsed.upstream_status,
                    upstream_error: parsed.upstream_error,
                    upstream_message: parsed.upstream_message,
                });
                slog!(error, "service", "storage_error",
                    code = e.code,
                    name = e.name,
                    error = %e,
                );
                Self::Detailed {
                    status,
                    kind,
                    message,
                    code: Some(e.code),
                    name: Some(e.name),
                    details: Some(details),
                }
            }
            _ => {
                slog!(error, "service", "internal_error",
                    code = e.code,
                    name = e.name,
                    error = %e,
                    span_trace = %tracing_error::SpanTrace::capture(),
                );
                Self::Detailed {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    kind: "internal",
                    message: e.message,
                    code: Some(e.code),
                    name: Some(e.name),
                    details: None,
                }
            }
        }
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::AgentNotFound(message) => (StatusCode::NOT_FOUND, ErrorEnvelope {
                error: ErrorPayload {
                    status: StatusCode::NOT_FOUND.as_u16(),
                    kind: "not_found",
                    message,
                    code: None,
                    name: None,
                    details: None,
                },
            }),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, ErrorEnvelope {
                error: ErrorPayload {
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    kind: "bad_request",
                    message,
                    code: None,
                    name: None,
                    details: None,
                },
            }),
            Self::Forbidden(message) => (StatusCode::FORBIDDEN, ErrorEnvelope {
                error: ErrorPayload {
                    status: StatusCode::FORBIDDEN.as_u16(),
                    kind: "forbidden",
                    message,
                    code: None,
                    name: None,
                    details: None,
                },
            }),
            Self::Conflict(message) => (StatusCode::CONFLICT, ErrorEnvelope {
                error: ErrorPayload {
                    status: StatusCode::CONFLICT.as_u16(),
                    kind: "conflict",
                    message,
                    code: None,
                    name: None,
                    details: None,
                },
            }),
            Self::RateLimited(message) => (StatusCode::TOO_MANY_REQUESTS, ErrorEnvelope {
                error: ErrorPayload {
                    status: StatusCode::TOO_MANY_REQUESTS.as_u16(),
                    kind: "rate_limited",
                    message,
                    code: None,
                    name: None,
                    details: None,
                },
            }),
            Self::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, ErrorEnvelope {
                error: ErrorPayload {
                    status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    kind: "internal",
                    message,
                    code: None,
                    name: None,
                    details: None,
                },
            }),
            Self::Detailed {
                status,
                kind,
                message,
                code,
                name,
                details,
            } => (status, ErrorEnvelope {
                error: ErrorPayload {
                    status: status.as_u16(),
                    kind,
                    message,
                    code,
                    name,
                    details,
                },
            }),
        };
        (status, Json(body)).into_response()
    }
}

pub type Result<T> = std::result::Result<T, ServiceError>;

fn parse_storage_error(message: &str) -> ParsedStorageError {
    let Some((operation, rest)) = message.split_once(": HTTP ") else {
        return ParsedStorageError {
            operation: None,
            upstream_status: None,
            upstream_error: None,
            upstream_message: Some(message.to_string()),
        };
    };

    let mut parsed = ParsedStorageError {
        operation: Some(operation.to_string()),
        upstream_status: None,
        upstream_error: None,
        upstream_message: None,
    };

    let Some((status_part, body)) = rest.split_once(": ") else {
        parsed.upstream_message = Some(rest.to_string());
        return parsed;
    };

    let status = status_part
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<u16>().ok());
    parsed.upstream_status = status;

    if let Ok(value) = serde_json::from_str::<StorageProxyErrorBody>(body) {
        if !value.error.is_empty() {
            parsed.upstream_error = Some(value.error);
        }
        if !value.message.is_empty() {
            parsed.upstream_message = Some(value.message);
        }
    }

    if parsed.upstream_message.is_none() && !body.is_empty() {
        parsed.upstream_message = Some(body.to_string());
    }

    parsed
}
