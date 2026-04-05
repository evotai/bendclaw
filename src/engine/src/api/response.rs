use reqwest::header::HeaderMap;
use serde_json::Value;

use super::ApiError;

pub async fn http_error(response: reqwest::Response) -> ApiError {
    let status = response.status().as_u16();
    let request_id = response_request_id(response.headers());
    let body = match response.text().await {
        Ok(body) => body,
        Err(error) => error.to_string(),
    };
    let message = attach_request_id(body, request_id);

    if status == 401 {
        return ApiError::AuthError(message);
    }

    if status == 429 {
        return ApiError::RateLimitError;
    }

    if message.contains("prompt is too long") {
        return ApiError::PromptTooLong(message);
    }

    ApiError::HttpError { status, message }
}

pub fn stream_error(value: &Value) -> Option<ApiError> {
    json_error_message(value).map(ApiError::StreamError)
}

fn json_error_message(value: &Value) -> Option<String> {
    if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
        return Some(message.to_string());
    }

    if let Some(message) = value
        .pointer("/error/error/message")
        .and_then(Value::as_str)
    {
        return Some(message.to_string());
    }

    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return Some(message.to_string());
    }

    None
}

fn response_request_id(headers: &HeaderMap) -> Option<String> {
    for key in [
        "x-request-id",
        "request-id",
        "openai-request-id",
        "anthropic-request-id",
        "x-amzn-requestid",
    ] {
        if let Some(value) = headers.get(key).and_then(|value| value.to_str().ok()) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

fn attach_request_id(message: String, request_id: Option<String>) -> String {
    match request_id {
        Some(request_id) if !request_id.is_empty() => {
            format!("{message} (request_id: {request_id})")
        }
        _ => message,
    }
}
