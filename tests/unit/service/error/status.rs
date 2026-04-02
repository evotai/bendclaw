use anyhow::Result;
use axum::body::to_bytes;
use axum::http::StatusCode;
use bendclaw::service::error::ServiceError;
use bendclaw::types::ErrorCode;
use serde_json::Value;

fn status(e: ServiceError) -> StatusCode {
    use axum::response::IntoResponse;
    e.into_response().status()
}

async fn body_json(e: ServiceError) -> Result<Value> {
    use axum::response::IntoResponse;
    let response = e.into_response();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&body)?)
}

#[test]
fn agent_not_found_is_404() -> Result<()> {
    assert_eq!(
        status(ServiceError::AgentNotFound("x".into())),
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[test]
fn forbidden_is_403() -> Result<()> {
    assert_eq!(
        status(ServiceError::Forbidden("x".into())),
        StatusCode::FORBIDDEN
    );
    Ok(())
}

#[test]
fn bad_request_is_400() -> Result<()> {
    assert_eq!(
        status(ServiceError::BadRequest("x".into())),
        StatusCode::BAD_REQUEST
    );
    Ok(())
}

#[test]
fn internal_is_500() -> Result<()> {
    assert_eq!(
        status(ServiceError::Internal("x".into())),
        StatusCode::INTERNAL_SERVER_ERROR,
    );
    Ok(())
}

#[test]
fn display_includes_message() -> Result<()> {
    let e = ServiceError::AgentNotFound("agent-42".into());
    assert!(e.to_string().contains("agent-42"));
    Ok(())
}

#[test]
fn from_error_code_internal_maps_to_500() -> Result<()> {
    let ec = ErrorCode::storage_exec("db down");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::INTERNAL_SERVER_ERROR);
    Ok(())
}

#[test]
fn from_error_code_not_found_preserves_message() -> Result<()> {
    let ec = ErrorCode::not_found("agent not initialized");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::NOT_FOUND);
    Ok(())
}

#[test]
fn from_error_code_skill_not_found_maps_to_404() -> Result<()> {
    let ec = ErrorCode::skill_not_found("skill foo missing");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::NOT_FOUND);
    Ok(())
}

#[test]
fn from_error_code_invalid_input_maps_to_400() -> Result<()> {
    let ec = ErrorCode::invalid_input("bad payload");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::BAD_REQUEST);
    Ok(())
}

#[test]
fn from_error_code_skill_validation_maps_to_400() -> Result<()> {
    let ec = ErrorCode::skill_validation("bad skill");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::BAD_REQUEST);
    Ok(())
}

#[test]
fn from_error_code_skill_requirements_maps_to_400() -> Result<()> {
    let ec = ErrorCode::skill_requirements("missing env");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::BAD_REQUEST);
    Ok(())
}

#[test]
fn from_error_code_denied_maps_to_403() -> Result<()> {
    let ec = ErrorCode::denied("not your session");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::FORBIDDEN);
    Ok(())
}

#[test]
fn from_error_code_rate_limit_maps_to_429() -> Result<()> {
    let ec = ErrorCode::llm_rate_limit("too many requests");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::TOO_MANY_REQUESTS);
    Ok(())
}

#[tokio::test]
async fn storage_error_body_is_structured() -> Result<()> {
    let ec = ErrorCode::storage_exec(
        r#"query: HTTP 500 Internal Server Error: {"error":"StorageProxyError","message":"Failed query on agentos"}"#,
    );
    let body = body_json(ec.into()).await?;

    assert_eq!(body["error"]["status"], 500);
    assert_eq!(body["error"]["kind"], "storage_exec");
    assert_eq!(body["error"]["message"], "storage query failed");
    assert_eq!(body["error"]["code"], 1101);
    assert_eq!(body["error"]["name"], "StorageExec");
    assert_eq!(body["error"]["details"]["type"], "storage");
    assert_eq!(body["error"]["details"]["operation"], "query");
    assert_eq!(body["error"]["details"]["upstream_status"], 500);
    assert_eq!(
        body["error"]["details"]["upstream_error"],
        "StorageProxyError"
    );
    assert_eq!(
        body["error"]["details"]["upstream_message"],
        "Failed query on agentos"
    );
    Ok(())
}
