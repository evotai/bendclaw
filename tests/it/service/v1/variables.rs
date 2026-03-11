//! Integration tests for the variables API.

use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::setup::json_body;
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::common::setup::TestContext;
use crate::mocks::llm::MockLLMProvider;

/// CRUD + secret masking in a single DB session.
#[tokio::test]
async fn variable_crud_and_secret_masking() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("var");
    let user = uid("u");
    setup_agent(&app, &agent_id, &user).await?;

    // Create non-secret
    let plain = serde_json::json!({ "key": "LOG_LEVEL", "value": "debug", "secret": false });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/variables"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&plain)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let created = json_body(resp).await?;
    assert_eq!(created["value"], "debug");
    let var_id = created["id"].as_str().context("missing id")?.to_string();

    // Create secret — value must be masked
    let secret = serde_json::json!({ "key": "API_KEY", "value": "real-secret", "secret": true });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/variables"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&secret)?))?,
        )
        .await?;
    let sec = json_body(resp).await?;
    assert_eq!(sec["value"], "****");
    assert_ne!(sec["value"], "real-secret");

    // List — secret still masked
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/variables"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let list = json_body(resp).await?;
    let items = list["data"].as_array().context("expected array")?;
    let api_key = items
        .iter()
        .find(|v| v["key"] == "API_KEY")
        .context("API_KEY missing")?;
    assert_eq!(api_key["value"], "****");

    // Delete
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}/variables/{var_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);

    // Get after delete → 404
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/variables/{var_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    Ok(())
}

/// Partial update: only changed fields are updated, others preserved.
#[tokio::test]
async fn variable_partial_update() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("var-upd");
    let user = uid("u");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "key": "TOKEN", "value": "tok123", "secret": false });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/variables"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    let created = json_body(resp).await?;
    let var_id = created["id"].as_str().context("missing id")?.to_string();

    // Promote to secret — key and value unchanged
    let patch = serde_json::json!({ "secret": true });
    app.clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/variables/{var_id}"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&patch)?))?,
        )
        .await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/variables/{var_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let body = json_body(resp).await?;
    assert_eq!(body["key"], "TOKEN"); // unchanged
    assert_eq!(body["secret"], true);
    assert_eq!(body["value"], "****"); // now masked
    Ok(())
}
