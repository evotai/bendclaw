use anyhow::Result;
use axum::http::StatusCode;
use bendclaw::service::error::ServiceError;

fn status(e: ServiceError) -> StatusCode {
    use axum::response::IntoResponse;
    e.into_response().status()
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
    let ec = bendclaw::base::ErrorCode::storage_exec("db down");
    let se: ServiceError = ec.into();
    assert!(matches!(se, ServiceError::Internal(_)));
    assert_eq!(status(se), StatusCode::INTERNAL_SERVER_ERROR);
    Ok(())
}

#[test]
fn from_error_code_not_found_preserves_message() -> Result<()> {
    let ec = bendclaw::base::ErrorCode::not_found("agent not initialized");
    let se: ServiceError = ec.into();
    assert!(matches!(se, ServiceError::AgentNotFound(_)));
    assert!(se.to_string().contains("agent not initialized"));
    Ok(())
}

#[test]
fn from_error_code_skill_not_found_maps_to_404() -> Result<()> {
    let ec = bendclaw::base::ErrorCode::skill_not_found("skill foo missing");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::NOT_FOUND);
    Ok(())
}

#[test]
fn from_error_code_invalid_input_maps_to_400() -> Result<()> {
    let ec = bendclaw::base::ErrorCode::invalid_input("bad payload");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::BAD_REQUEST);
    Ok(())
}

#[test]
fn from_error_code_denied_maps_to_403() -> Result<()> {
    let ec = bendclaw::base::ErrorCode::denied("not your session");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::FORBIDDEN);
    Ok(())
}

#[test]
fn from_error_code_rate_limit_maps_to_429() -> Result<()> {
    let ec = bendclaw::base::ErrorCode::llm_rate_limit("too many requests");
    let se: ServiceError = ec.into();
    assert_eq!(status(se), StatusCode::TOO_MANY_REQUESTS);
    Ok(())
}
