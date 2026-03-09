use anyhow::Context;
use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::setup::TestContext;
use crate::common::setup::json_body;
use crate::common::setup::uid;

#[tokio::test]
async fn health_check_returns_ok() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app().await?;
    let resp = app
        .oneshot(Request::builder().uri("/health").body(Body::empty())?)
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = json_body(resp).await?;
    assert!(json["status"].is_string());
    assert!(json["checks"]["service"]["ok"].as_bool().unwrap_or(false));
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn setup_agent_creates_database() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app().await?;
    let user = uid("user");
    let agent_id = uid("agent");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/setup"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["ok"], true);
    assert!(body["database"].as_str().is_some());
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn missing_auth_headers_return_401() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app().await?;
    let agent_id = uid("agent");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/setup"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = json_body(resp).await?;
    let err = body["error"]
        .as_str()
        .context("missing error message for missing user header")?;
    assert!(err.contains("x-user-id"));
    ctx.teardown().await;
    Ok(())
}
